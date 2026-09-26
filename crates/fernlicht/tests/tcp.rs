use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use fernlicht::transport::{self, ClientOptions, Framing, Incoming, UdsLink};
use fernlicht::uds::read_did;

/// A gateway on localhost that answers any read with a VIN, one byte per
/// write so the client has to reassemble the frame.
fn gateway() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut rx = Vec::new();
        let mut buf = [0; 256];
        while let Ok(n @ 1..) = stream.read(&mut buf) {
            rx.extend_from_slice(&buf[..n]);
            while let Ok(Some((msg, size))) = Framing::Hsfz.parse(&rx) {
                rx.drain(..size);
                let Incoming::Uds { src, dst, uds } = msg else { continue };
                if uds[0] != 0x22 {
                    continue;
                }
                let mut reply = vec![0x62, 0xf1, 0x90];
                reply.extend_from_slice(b"WBA00000000000042");
                for byte in Framing::Hsfz.frame(dst, src, &reply) {
                    stream.write_all(&[byte]).unwrap();
                }
            }
        }
    });
    port
}

#[test]
fn reassembles_a_reply_sent_byte_by_byte() {
    let port = gateway();
    let options = ClientOptions { keep_alive: None, ..ClientOptions::default() };
    let client = transport::connect_port("127.0.0.1", port, Framing::Hsfz, &options).unwrap();
    let reply = client.request(0x10, &read_did(0xf190), Duration::from_secs(2)).unwrap();
    assert_eq!(&reply.data().unwrap()[2..], b"WBA00000000000042");
}

#[test]
fn connect_reports_every_framing_it_tried() {
    // Bind and drop to get a port nothing listens on.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err =
        transport::connect_port("127.0.0.1", port, Framing::Hsfz, &ClientOptions::default()).unwrap_err();
    assert!(matches!(err, fernlicht::Error::Io(_)), "{err}");

    let err = transport::connect("127.0.0.1", &[], &ClientOptions::default()).unwrap_err();
    assert_eq!(err.to_string(), "no answer from 127.0.0.1");
}
