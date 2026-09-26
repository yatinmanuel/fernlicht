use std::collections::HashSet;
use std::fmt;
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{Framing, Incoming, Transport, UdsLink};
use crate::uds::{self, Reply};
use crate::{Error, Result};

/// Where keep-alives go. Any module would do; the gateway is always there.
const KEEP_ALIVE_TARGET: u16 = 0x10;

const READ_CHUNK: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

/// Called with every UDS message that goes out or comes in.
pub type Tracer = Arc<dyn Fn(Direction, u16, &[u8]) + Send + Sync>;

#[derive(Clone)]
pub struct ClientOptions {
    /// Limit for the TCP connect and the DoIP routing activation.
    pub connect_timeout: Duration,
    /// How often to send TesterPresent while idle. Forced outputs only hold
    /// while the diagnostic session lives, and it dies a few seconds after the
    /// last request. `None` disables it.
    pub keep_alive: Option<Duration>,
    /// Ceiling for a module that keeps answering "response pending".
    pub max_pending: Duration,
    pub trace: Option<Tracer>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(4),
            keep_alive: Some(Duration::from_secs(2)),
            max_pending: Duration::from_secs(30),
            trace: None,
        }
    }
}

impl fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientOptions")
            .field("connect_timeout", &self.connect_timeout)
            .field("keep_alive", &self.keep_alive)
            .field("max_pending", &self.max_pending)
            .field("trace", &self.trace.is_some())
            .finish()
    }
}

/// A diagnostic connection to the gateway.
///
/// Requests go out one at a time; callers on other threads wait their turn.
/// After a fatal error the client stays closed and every request fails with
/// [`Error::Closed`]; connect again to continue.
pub struct Client {
    framing: Framing,
    shared: Arc<Shared>,
    keep_alive: Option<JoinHandle<()>>,
}

struct Shared {
    conn: Mutex<Connection>,
    shutdown: Mutex<bool>,
    wake: Condvar,
}

struct Connection {
    framing: Framing,
    /// `None` once closed.
    transport: Option<Box<dyn Transport>>,
    rx: Vec<u8>,
    /// Requests that timed out, keyed on what their reply would echo. Keying
    /// on the echo rather than the whole request means a timed out "start
    /// routine" does not block the "stop routine" that cleans up after it.
    stale: HashSet<(u16, Vec<u8>)>,
    last_sent: Instant,
    max_pending: Duration,
    trace: Option<Tracer>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Client {
    /// Takes over an open transport and performs the framing's handshake.
    pub fn new(
        transport: impl Transport + 'static,
        framing: Framing,
        options: &ClientOptions,
    ) -> Result<Self> {
        let mut conn = Connection {
            framing,
            transport: Some(Box::new(transport)),
            rx: Vec::new(),
            stale: HashSet::new(),
            last_sent: Instant::now(),
            max_pending: options.max_pending,
            trace: options.trace.clone(),
        };
        if let Some(hello) = framing.hello() {
            conn.activate(&hello, options.connect_timeout)?;
        }

        let shared =
            Arc::new(Shared { conn: Mutex::new(conn), shutdown: Mutex::new(false), wake: Condvar::new() });
        let keep_alive = options.keep_alive.filter(|every| !every.is_zero()).map(|every| {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("fernlicht-keepalive".into())
                .spawn(move || keep_alive(&shared, every))
        });
        Ok(Self { framing, shared, keep_alive: keep_alive.transpose()? })
    }

    pub fn framing(&self) -> Framing {
        self.framing
    }

    pub fn is_open(&self) -> bool {
        lock(&self.shared.conn).transport.is_some()
    }

    /// Closes the connection. Pending and later requests fail with [`Error::Closed`].
    pub fn close(&self) {
        lock(&self.shared.conn).transport = None;
        self.stop_keep_alive();
    }

    fn stop_keep_alive(&self) {
        *lock(&self.shared.shutdown) = true;
        self.shared.wake.notify_all();
    }
}

impl UdsLink for Client {
    fn request(&self, ecu: u16, request: &[u8], timeout: Duration) -> Result<Reply> {
        let mut conn = lock(&self.shared.conn);
        let result = conn.request(ecu, request, timeout);
        if result.as_ref().is_err_and(Error::is_fatal) {
            conn.transport = None;
        }
        result
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop_keep_alive();
        if let Some(handle) = self.keep_alive.take() {
            let _ = handle.join();
        }
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("framing", &self.framing)
            .field("open", &self.is_open())
            .finish_non_exhaustive()
    }
}

fn keep_alive(shared: &Shared, every: Duration) {
    loop {
        {
            let shutdown = lock(&shared.shutdown);
            let (shutdown, _) = shared
                .wake
                .wait_timeout_while(shutdown, every, |stop| !*stop)
                .unwrap_or_else(PoisonError::into_inner);
            if *shutdown {
                return;
            }
        }
        // A request in flight keeps the session alive by itself.
        if let Ok(mut conn) = shared.conn.try_lock() {
            if conn.transport.is_some() && conn.keep_alive(every).is_err() {
                conn.transport = None;
            }
        }
    }
}

impl Connection {
    fn request(&mut self, ecu: u16, request: &[u8], timeout: Duration) -> Result<Reply> {
        if self.transport.is_none() {
            return Err(Error::Closed);
        }
        if request.is_empty() {
            return Err(Error::InvalidRequest("empty request"));
        }
        if ecu > self.framing.max_ecu() {
            return Err(Error::InvalidRequest("module address out of range"));
        }
        if timeout.is_zero() {
            return Err(Error::InvalidRequest("zero timeout"));
        }
        let key = (ecu, uds::signature(request).to_vec());
        if self.stale.contains(&key) {
            return Err(Error::Stale { ecu });
        }

        self.send_uds(ecu, request)?;
        let give_up = Instant::now() + self.max_pending;
        let mut deadline = (Instant::now() + timeout).min(give_up);
        loop {
            while let Some(msg) = self.next_message()? {
                let Incoming::Uds { src, dst, uds: response } = msg else {
                    self.background(msg)?;
                    continue;
                };
                self.trace(Direction::Received, src, &response);
                if src != ecu || dst != self.framing.tester() || !uds::answers(request, &response) {
                    continue;
                }
                if uds::is_pending(&response) {
                    deadline = (Instant::now() + timeout).min(give_up);
                } else if let Some(reply) = Reply::parse(&response) {
                    return Ok(reply);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                self.stale.insert(key);
                return Err(Error::Timeout { ecu });
            }
            self.fill(deadline - now)?;
        }
    }

    fn activate(&mut self, hello: &[u8], timeout: Duration) -> Result<()> {
        self.send_raw(hello)?;
        let deadline = Instant::now() + timeout;
        loop {
            while let Some(msg) = self.next_message()? {
                match msg {
                    Incoming::Activated => return Ok(()),
                    other => self.background(other)?,
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::ActivationTimeout);
            }
            self.fill(deadline - now)?;
        }
    }

    fn keep_alive(&mut self, every: Duration) -> Result<()> {
        if self.last_sent.elapsed() >= every {
            let frame =
                self.framing.frame(self.framing.tester(), KEEP_ALIVE_TARGET, &uds::tester_present(true));
            self.send_raw(&frame)?;
        }
        // Answer alive checks and drop late replies that piled up while idle.
        self.fill(Duration::from_millis(1))?;
        while let Some(msg) = self.next_message()? {
            if let Incoming::Uds { src, uds, .. } = &msg {
                self.trace(Direction::Received, *src, uds);
            } else {
                self.background(msg)?;
            }
        }
        Ok(())
    }

    /// Handles a message that is not the reply being waited for.
    fn background(&mut self, msg: Incoming) -> Result<()> {
        match msg {
            Incoming::AliveCheck => match self.framing.alive_reply() {
                Some(reply) => self.send_raw(&reply),
                None => Ok(()),
            },
            Incoming::Refused(reason) => Err(Error::Refused(reason)),
            Incoming::Uds { .. } | Incoming::Activated | Incoming::Ignored => Ok(()),
        }
    }

    fn next_message(&mut self) -> Result<Option<Incoming>> {
        let Some((msg, size)) = self.framing.parse(&self.rx)? else {
            return Ok(None);
        };
        self.rx.drain(..size);
        Ok(Some(msg))
    }

    /// Reads once into the receive buffer, waiting at most `timeout`.
    fn fill(&mut self, timeout: Duration) -> Result<()> {
        let transport = self.transport.as_mut().ok_or(Error::Closed)?;
        let mut chunk = [0; READ_CHUNK];
        match transport.recv(&mut chunk, timeout) {
            Ok(0) => Err(Error::Closed),
            Ok(n) => {
                self.rx.extend_from_slice(&chunk[..n]);
                Ok(())
            }
            Err(err) if is_timeout(&err) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    fn send_uds(&mut self, ecu: u16, request: &[u8]) -> Result<()> {
        self.trace(Direction::Sent, ecu, request);
        let frame = self.framing.frame(self.framing.tester(), ecu, request);
        self.send_raw(&frame)
    }

    fn send_raw(&mut self, bytes: &[u8]) -> Result<()> {
        let transport = self.transport.as_mut().ok_or(Error::Closed)?;
        transport.send(bytes)?;
        self.last_sent = Instant::now();
        Ok(())
    }

    fn trace(&self, direction: Direction, ecu: u16, uds: &[u8]) {
        if let Some(trace) = &self.trace {
            trace(direction, ecu, uds);
        }
    }
}

fn is_timeout(err: &io::Error) -> bool {
    matches!(err.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted)
}
