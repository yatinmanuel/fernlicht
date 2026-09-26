mod common;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use common::{FakeCar, ascii, did_reply};
use fernlicht::Error;
use fernlicht::bmw::{self, Answer, Guard, Lamp, LightProbe, Profile, RemOutput, ScanOptions};
use fernlicht::transport::UdsLink;
use fernlicht::uds::{self, Nrc, Reply, Session};

/// An F82 with LED headlights, as far as identification goes.
fn f82() -> FakeCar {
    let names = BTreeMap::from([
        (0x10, "ZGW_01"),
        (0x40, "FEM_20"),
        (0x43, "FLE02_L"),
        (0x44, "FLE02_R"),
        (0x72, "REM_20"),
    ]);
    FakeCar::new(move |ecu, request| {
        let Some(name) = names.get(&ecu) else {
            return vec![];
        };
        let reply = match request {
            [0x3e, ..] => vec![0x7e, 0x00],
            [0x22, 0xf1, 0x90] => did_reply(0xf190, &ascii("WBA00000000000042")),
            [0x22, 0xf1, 0x97] => did_reply(0xf197, &ascii(name)),
            [0x22, ..] => vec![0x7f, 0x22, 0x31],
            [sid, ..] => vec![0x7f, *sid, 0x11],
            [] => return vec![],
        };
        vec![reply]
    })
}

#[test]
fn identify_only_reads() {
    let car = f82();
    let mut seen = Vec::new();
    let report = bmw::identify(&car.client(), ScanOptions::default(), |m| seen.push(m.address));

    assert_eq!(report.vin.as_deref(), Some("WBA00000000******"));
    assert_eq!(
        report.profile,
        Profile { body: Some(0x40), left: Some(0x43), right: Some(0x44), rear: Some(0x72) }
    );
    let names: Vec<_> = report.modules.iter().map(|m| m.name.as_deref().unwrap()).collect();
    assert_eq!(names, ["ZGW_01", "FEM_20", "FLE02_L", "FLE02_R", "REM_20"]);
    assert_eq!(seen, [0x10, 0x40, 0x43, 0x44, 0x72]);

    let services: BTreeSet<String> =
        car.sent().iter().map(|line| line.split(' ').nth(1).unwrap().to_owned()).collect();
    assert_eq!(services, BTreeSet::from(["22".to_owned(), "3e".to_owned()]));
}

#[test]
fn unmapped_car_still_produces_a_useful_report() {
    // G-series shaped: a BDC at 0x40 that knows neither light DID, and no FLE.
    let car = FakeCar::new(|ecu, request| {
        if ecu != 0x10 && ecu != 0x40 {
            return vec![];
        }
        let reply = match request {
            [0x3e, ..] => vec![0x7e, 0x00],
            [0x31, ..] => vec![0x7f, 0x31, 0x31],
            [0x22, 0xf1, 0x97] => did_reply(0xf197, &ascii(if ecu == 0x40 { "BDC_BODY" } else { "ZGW_02" })),
            [0x22, 0xf1, 0x50] => did_reply(0xf150, &[0x0f, 0x2b, 0x40]),
            _ => vec![0x7f, 0x22, 0x31],
        };
        vec![reply]
    });
    let report = bmw::identify(
        &car.client(),
        ScanOptions { lights: true, keep_vin: true, ..Default::default() },
        |_| {},
    );

    assert_eq!(report.vin, None);
    assert_eq!(report.profile, Profile::default());
    let bdc = report.modules.iter().find(|m| m.address == 0x40).unwrap();
    assert_eq!(bdc.name.as_deref(), Some("BDC_BODY"));
    assert_eq!(bdc.ids["f150"].to_string(), "0f 2b 40");
    let out_of_range = Answer::Negative(Nrc::REQUEST_OUT_OF_RANGE);
    assert_eq!(
        bdc.lights,
        Some(LightProbe {
            lamp_function: out_of_range.clone(),
            lamp_output: out_of_range.clone(),
            led_routine: out_of_range
        })
    );
    // Probing asks; it never writes or starts anything.
    for line in car.sent() {
        assert!(line.contains(": 22 ") || line.contains(": 3e ") || line.contains(": 31 03 "), "{line}");
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_serialises_to_the_documented_shape() {
    let report = bmw::identify(&f82().client(), ScanOptions { lights: true, ..Default::default() }, |_| {});
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["framing"], "hsfz");
    assert_eq!(json["vin"], "WBA00000000******");
    assert_eq!(json["profile"]["body"], 0x40);
    assert_eq!(json["modules"][1]["name"], "FEM_20");
    assert_eq!(json["modules"][1]["lights"]["lampFunction"], "nrc 31");
    assert!(json["createdAt"].as_str().unwrap().ends_with('Z'));
}

struct Recorder(RefCell<Vec<String>>);

impl UdsLink for Recorder {
    fn request(&self, ecu: u16, request: &[u8], _: Duration) -> fernlicht::Result<Reply> {
        self.0.borrow_mut().push(format!("{ecu:x}: {}", fernlicht::bytes::Hex(request)));
        Ok(Reply::Positive { sid: request[0], data: vec![] })
    }
}

#[test]
fn guard_passes_known_light_commands_and_nothing_else() {
    let recorder = Recorder(RefCell::default());
    let profile = Profile { body: Some(0x40), left: Some(0x43), right: Some(0x44), rear: None };
    let link = Guard::new(&recorder, profile);

    link.send(0x40, &bmw::fem_lamp(Lamp::Drl, Duration::from_millis(100))).unwrap();
    link.send(0x43, &bmw::fle_leds(50)).unwrap();
    link.send(0x44, &bmw::fle_stop()).unwrap();
    link.send(0x60, &uds::read_did(0xf190)).unwrap();
    link.send(0x40, &uds::session_control(Session::Extended)).unwrap();

    let blocked = |ecu, request: Vec<u8>| matches!(link.send(ecu, &request), Err(Error::Blocked { .. }));
    assert!(blocked(0x40, vec![0x10, 0x02]));
    assert!(blocked(0x40, uds::write_did(0x1234, &[1])));
    assert!(blocked(0x60, bmw::fem_lamp(Lamp::Drl, Duration::from_millis(100))));
    assert!(blocked(0x72, bmw::rem_output(RemOutput::Plate, false)));
    assert!(blocked(0x40, vec![0x11, 0x01]));
    assert!(blocked(0x43, uds::routine(uds::Routine::Start, 0x1234, &[])));
    assert!(blocked(0x40, vec![]));
    assert_eq!(recorder.0.borrow().len(), 5);
}
