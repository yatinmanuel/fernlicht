//! An in-memory gateway for tests.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use fernlicht::bytes::Hex;
use fernlicht::transport::{Client, ClientOptions, Framing, Incoming, Transport};

type Responder = Box<dyn FnMut(u16, &[u8]) -> Vec<Vec<u8>> + Send>;

/// Decodes what the client sends and answers through a responder. The
/// responder returns the replies to send, none for a silent address.
#[derive(Clone)]
pub struct FakeCar(Arc<(Mutex<State>, Condvar)>);

struct State {
    inbox: VecDeque<u8>,
    sent: Vec<String>,
    closed: bool,
    respond: Responder,
}

impl FakeCar {
    pub fn new(respond: impl FnMut(u16, &[u8]) -> Vec<Vec<u8>> + Send + 'static) -> Self {
        let state =
            State { inbox: VecDeque::new(), sent: Vec::new(), closed: false, respond: Box::new(respond) };
        Self(Arc::new((Mutex::new(state), Condvar::new())))
    }

    /// A car where nothing answers.
    pub fn silent() -> Self {
        Self::new(|_, _| Vec::new())
    }

    /// Opens a client on this car without a keep-alive thread.
    pub fn client(&self) -> Client {
        let options = ClientOptions { keep_alive: None, ..ClientOptions::default() };
        Client::new(self.clone(), Framing::Hsfz, &options).expect("hsfz has no handshake")
    }

    /// Every request received, as `"ecu: bytes"`, keep-alives left out.
    pub fn sent(&self) -> Vec<String> {
        self.0.0.lock().unwrap().sent.clone()
    }

    /// Queues a reply as if `ecu` had sent it unprompted.
    pub fn push(&self, ecu: u16, uds: &[u8]) {
        self.push_raw(&Framing::Hsfz.frame(ecu, Framing::Hsfz.tester(), uds));
    }

    /// Queues bytes exactly as given.
    pub fn push_raw(&self, bytes: &[u8]) {
        let (state, arrived) = &*self.0;
        state.lock().unwrap().inbox.extend(bytes);
        arrived.notify_all();
    }

    pub fn hang_up(&self) {
        let (state, arrived) = &*self.0;
        state.lock().unwrap().closed = true;
        arrived.notify_all();
    }
}

impl Transport for FakeCar {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        let (state, arrived) = &*self.0;
        let mut state = state.lock().unwrap();
        if state.closed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let Ok(Some((Incoming::Uds { dst, uds, .. }, _))) = Framing::Hsfz.parse(bytes) else {
            return Ok(());
        };
        if uds == [0x3e, 0x80] {
            return Ok(());
        }
        state.sent.push(format!("{dst:x}: {}", Hex(&uds)));
        for reply in (state.respond)(dst, &uds) {
            state.inbox.extend(Framing::Hsfz.frame(dst, Framing::Hsfz.tester(), &reply));
        }
        arrived.notify_all();
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let (state, arrived) = &*self.0;
        let state = state.lock().unwrap();
        let (mut state, _) =
            arrived.wait_timeout_while(state, timeout, |s| s.inbox.is_empty() && !s.closed).unwrap();
        if state.inbox.is_empty() {
            return if state.closed { Ok(0) } else { Err(io::ErrorKind::TimedOut.into()) };
        }
        let n = buf.len().min(state.inbox.len());
        for (slot, byte) in buf.iter_mut().zip(state.inbox.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

pub fn ascii(text: &str) -> Vec<u8> {
    text.bytes().collect()
}

/// A positive reply to a read of `did` carrying `data`.
pub fn did_reply(did: u16, data: &[u8]) -> Vec<u8> {
    let mut reply = vec![0x62];
    reply.extend_from_slice(&did.to_be_bytes());
    reply.extend_from_slice(data);
    reply
}
