mod common;

use std::thread;
use std::time::Duration;

use common::FakeCar;
use fernlicht::Error;
use fernlicht::bytes::{Hex, parse_hex};
use fernlicht::transport::UdsLink;
use fernlicht::uds::{Nrc, Reply, Routine, read_did, routine};

const SHORT: Duration = Duration::from_millis(30);
const LONG: Duration = Duration::from_secs(2);

#[test]
fn request_returns_the_parsed_reply() {
    let car = FakeCar::new(|_, _| vec![vec![0x62, 0xf1, 0x90, 0x41, 0x42]]);
    let reply = car.client().request(0x10, &read_did(0xf190), LONG).unwrap();
    assert_eq!(reply, Reply::Positive { sid: 0x22, data: vec![0xf1, 0x90, 0x41, 0x42] });
}

#[test]
fn negative_reply_carries_the_nrc() {
    let car = FakeCar::new(|_, _| vec![vec![0x7f, 0x22, 0x31]]);
    let reply = car.client().request(0x40, &read_did(0x1234), LONG).unwrap();
    assert_eq!(reply.nrc(), Some(Nrc::REQUEST_OUT_OF_RANGE));
}

#[test]
fn send_turns_negative_into_error() {
    let car = FakeCar::new(|_, _| vec![vec![0x7f, 0x22, 0x31]]);
    let err = car.client().send(0x40, &read_did(0x1234)).unwrap_err();
    assert!(matches!(err, Error::Negative { ecu: 0x40, nrc: Nrc::REQUEST_OUT_OF_RANGE }));
}

#[test]
fn response_pending_keeps_waiting() {
    let car = FakeCar::new(|_, _| vec![vec![0x7f, 0x22, 0x78], vec![0x62, 0xf1, 0x90]]);
    assert!(car.client().request(0x10, &read_did(0xf190), LONG).unwrap().is_positive());
}

#[test]
fn replies_from_another_module_or_for_another_did_are_skipped() {
    let car = FakeCar::silent();
    let client = car.client();
    car.push(0x43, &[0x62, 0xf1, 0x90]);
    car.push(0x40, &[0x62, 0xf1, 0x91]);
    car.push(0x40, &[0x62, 0xf1, 0x90, 0x01]);
    let reply = client.request(0x40, &read_did(0xf190), LONG).unwrap();
    assert_eq!(Hex(reply.data().unwrap()).to_string(), "f1 90 01");
}

#[test]
fn timed_out_request_is_not_retried_on_the_same_connection() {
    let client = FakeCar::silent().client();
    assert!(matches!(client.request(0x55, &read_did(0xf190), SHORT), Err(Error::Timeout { ecu: 0x55 })));
    assert!(matches!(client.request(0x55, &read_did(0xf190), SHORT), Err(Error::Stale { ecu: 0x55 })));
    // A different request to the same module is fine.
    assert!(matches!(client.request(0x55, &read_did(0xf191), SHORT), Err(Error::Timeout { .. })));
}

#[test]
fn timed_out_start_does_not_block_the_stop() {
    let car = FakeCar::new(|_, uds| if uds[1] == 0x01 { vec![] } else { vec![vec![0x71, 0x02, 0x30, 0x00]] });
    let client = car.client();
    let start = |data| routine(Routine::Start, 0x3000, data);
    assert!(matches!(client.request(0x43, &start(&[1, 2]), SHORT), Err(Error::Timeout { .. })));
    assert!(matches!(client.request(0x43, &start(&[3, 4]), SHORT), Err(Error::Stale { .. })));
    assert!(client.request(0x43, &routine(Routine::Stop, 0x3000, &[]), LONG).unwrap().is_positive());
}

#[test]
fn concurrent_requests_each_get_their_own_reply() {
    let car = FakeCar::new(|_, uds| vec![vec![0x62, uds[1], uds[2]]]);
    let client = car.client();
    thread::scope(|scope| {
        for did in 1..=8u16 {
            let client = &client;
            scope.spawn(move || {
                let reply = client.request(0x40, &read_did(did), LONG).unwrap();
                assert_eq!(reply.data().unwrap(), did.to_be_bytes());
            });
        }
    });
    assert_eq!(car.sent().len(), 8);
}

#[test]
fn hang_up_fails_the_pending_request_and_closes_the_client() {
    let car = FakeCar::silent();
    let client = car.client();
    thread::scope(|scope| {
        let pending = scope.spawn(|| client.request(0x40, &read_did(1), LONG));
        thread::sleep(Duration::from_millis(50));
        car.hang_up();
        assert!(matches!(pending.join().unwrap(), Err(Error::Closed)));
    });
    assert!(!client.is_open());
    assert!(matches!(client.request(0x40, &read_did(2), LONG), Err(Error::Closed)));
}

#[test]
fn invalid_requests_are_refused_locally() {
    let car = FakeCar::silent();
    let client = car.client();
    assert!(matches!(client.request(0x40, &[], LONG), Err(Error::InvalidRequest(_))));
    assert!(matches!(client.request(0x100, &read_did(1), LONG), Err(Error::InvalidRequest(_))));
    assert!(car.sent().is_empty());
}

#[test]
fn gateway_error_closes_the_connection() {
    let car = FakeCar::silent();
    let client = car.client();
    car.push_raw(&parse_hex("00 00 00 02 00 43 f4 99").unwrap());
    let err = client.request(0x99, &read_did(1), LONG).unwrap_err();
    assert_eq!(err.to_string(), "hsfz: incorrect destination address");
    assert!(!client.is_open());
}
