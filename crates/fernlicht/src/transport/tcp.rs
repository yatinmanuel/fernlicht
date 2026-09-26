use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use super::{Client, ClientOptions, Framing, Transport, doip_identification_request};
use crate::{Error, Result};

/// F-series gateways speak HSFZ, G-series DoIP. HSFZ first because a refused
/// port fails fast while an unanswered DoIP activation waits for the timeout.
pub const DEFAULT_ORDER: [Framing; 2] = [Framing::Hsfz, Framing::Doip];

/// Where [`discover`] shouts. The second address covers link-local ENET cables
/// on hosts whose default route points elsewhere.
const BROADCAST: [Ipv4Addr; 2] = [Ipv4Addr::BROADCAST, Ipv4Addr::new(169, 254, 255, 255)];

/// Single datagrams get lost on the Wi-Fi adapters, so the request goes out
/// several times.
const REPEAT_AT_MS: [u64; 4] = [0, 400, 1000, 1800];

impl Transport for TcpStream {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_all(bytes)
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        // A zero read timeout means "block forever" to the OS.
        self.set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        self.read(buf)
    }
}

/// Connects to the gateway at `host`, trying each framing in `order` on its
/// default port until one works.
pub fn connect(host: &str, order: &[Framing], options: &ClientOptions) -> Result<Client> {
    let mut attempts = Vec::new();
    for &framing in order {
        match connect_port(host, framing.port(), framing, options) {
            Ok(client) => return Ok(client),
            Err(err) => attempts.push((framing, err)),
        }
    }
    Err(Error::Unreachable { host: host.to_owned(), attempts })
}

/// Connects with one framing on an explicit port.
pub fn connect_port(host: &str, port: u16, framing: Framing, options: &ClientOptions) -> Result<Client> {
    let mut last = None;
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, options.connect_timeout) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Client::new(stream, framing, options);
            }
            Err(err) => last = Some(err),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "host did not resolve")).into())
}

/// Broadcasts a DoIP vehicle identification request and returns every address
/// that answered within `timeout`.
///
/// Only DoIP gateways answer, but the ENET adapters that sit in front of
/// F-series cars usually do too.
pub fn discover(timeout: Duration) -> io::Result<Vec<IpAddr>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    let request = doip_identification_request();
    let start = Instant::now();
    let mut repeats =
        REPEAT_AT_MS.iter().map(|&ms| Duration::from_millis(ms)).filter(|at| *at < timeout).peekable();
    let mut found = Vec::new();
    let mut buf = [0; 512];

    loop {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return Ok(found);
        }
        while repeats.next_if(|at| *at <= elapsed).is_some() {
            for addr in BROADCAST {
                // One unreachable broadcast address must not stop the other.
                let _ = socket.send_to(&request, (addr, Framing::Doip.port()));
            }
        }
        let wake = repeats.peek().copied().unwrap_or(timeout).min(timeout);
        socket.set_read_timeout(Some(wake.saturating_sub(elapsed).max(Duration::from_millis(1))))?;
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let is_doip = n >= 8 && buf[0] == !buf[1];
                if is_doip && !found.contains(&from.ip()) {
                    found.push(from.ip());
                }
            }
            Err(err) if matches!(err.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(err) => return Err(err),
        }
    }
}
