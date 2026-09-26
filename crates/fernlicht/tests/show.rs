use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use fernlicht::Error;
use fernlicht::bmw::Profile;
use fernlicht::bytes::Hex;
use fernlicht::show::{self, Driver, Levels, Lights, Output, Player, Show, Step, Via, driver_for};
use fernlicht::transport::UdsLink;
use fernlicht::uds::{Nrc, Reply};

/// Records requests and answers them positively, or negatively where `fail` says so.
struct Recorder {
    sent: RefCell<Vec<String>>,
    fail: fn(u16, &[u8]) -> bool,
}

impl Recorder {
    fn new() -> Self {
        Self { sent: RefCell::default(), fail: |_, _| false }
    }

    fn sent(&self) -> Vec<String> {
        self.sent.borrow().clone()
    }
}

impl UdsLink for Recorder {
    fn request(&self, ecu: u16, request: &[u8], _: Duration) -> fernlicht::Result<Reply> {
        self.sent.borrow_mut().push(format!("{ecu:x}: {}", Hex(request)));
        if (self.fail)(ecu, request) {
            return Ok(Reply::Negative { sid: request[0], nrc: Nrc::CONDITIONS_NOT_CORRECT });
        }
        Ok(Reply::Positive { sid: request[0], data: vec![] })
    }
}

fn quick(via: Via, repeat: bool) -> Show {
    Show {
        id: "quick".into(),
        name: "Quick".into(),
        via,
        repeat,
        steps: vec![
            Step::new(Levels::new().with(&[Output::FlLow], 255), 1),
            Step::new(Levels::new().with(&[Output::FlLow], 0).with(&[Output::RlBrake], 255), 1),
        ],
    }
}

const FEM: Profile = Profile { body: Some(0x40), left: None, right: None, rear: None };
const FLE: Profile = Profile { body: None, left: Some(0x43), right: Some(0x44), rear: None };

#[test]
fn builtin_shows_are_well_formed() {
    let shows = show::builtin();
    let mut ids: Vec<_> = shows.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), shows.len());
    for show in &shows {
        assert!(!show.steps.is_empty(), "{}", show.id);
        assert!(!show.repeat || show.steps.iter().any(|s| !s.hold.is_zero()), "{}", show.id);
    }
}

#[test]
fn fle_shows_stay_on_the_front() {
    for show in show::builtin().iter().filter(|s| s.via == Via::Fle) {
        for step in &show.steps {
            assert!(step.levels.iter().all(|(output, _)| output.is_front()), "{}", show.id);
        }
    }
}

#[test]
fn fem_driver_sends_lamp_functions_and_clears_them() {
    let link = Recorder::new();
    let mut driver = driver_for(&quick(Via::Fem, false), &link, &FEM).unwrap();
    Player::new().play(&quick(Via::Fem, false), &mut driver).unwrap();
    assert_eq!(
        link.sent(),
        ["40: 10 03", "40: 2e d5 42 00 03 00 0c", "40: 2e d5 42 00 0c 00 0c", "40: 2e d5 42 00 00 00 00"]
    );
}

#[test]
fn fle_driver_scales_levels_to_pwm_per_side() {
    let link = Recorder::new();
    let show = Show {
        steps: vec![Step::new(Levels::new().with(&[Output::FlDrl], 255).with(&[Output::FrDrl], 128), 1)],
        ..quick(Via::Fle, false)
    };
    let mut driver = driver_for(&show, &link, &FLE).unwrap();
    Player::new().play(&show, &mut driver).unwrap();
    let sent = link.sent();
    assert!(sent[2].starts_with("43: 31 01 30 00 32 64 "), "{}", sent[2]);
    assert!(sent[3].starts_with("44: 31 01 30 00 32 32 "), "{}", sent[3]);
    assert_eq!(sent[4..], ["43: 31 02 30 00", "44: 31 02 30 00"]);
}

#[test]
fn show_refuses_to_start_without_its_modules() {
    let link = Recorder::new();
    let breathe = show::find("breathe").unwrap();
    assert!(matches!(driver_for(&breathe, &link, &FEM), Err(Error::MissingModule { .. })));
    assert!(link.sent().is_empty());
}

#[test]
fn lamps_are_released_when_a_frame_fails() {
    let link = Recorder { fail: |_, request| request[0] == 0x2e && request[4] == 0x0c, ..Recorder::new() };
    let mut driver = driver_for(&quick(Via::Fem, false), &link, &FEM).unwrap();
    let err = Player::new().play(&quick(Via::Fem, false), &mut driver).unwrap_err();
    assert!(matches!(err, Error::Negative { nrc: Nrc::CONDITIONS_NOT_CORRECT, .. }));
    assert_eq!(link.sent().last().unwrap(), "40: 2e d5 42 00 00 00 00");
}

#[test]
fn failed_release_is_reported_as_not_restored() {
    let link = Recorder { fail: |_, request| request == [0x31, 0x02, 0x30, 0x00], ..Recorder::new() };
    let mut driver = driver_for(&quick(Via::Fle, false), &link, &FLE).unwrap();
    let err = Player::new().play(&quick(Via::Fle, false), &mut driver).unwrap_err();
    assert!(matches!(err, Error::NotRestored(_)));
    // Both headlights were asked to stop even though the first refused.
    assert!(link.sent().ends_with(&["43: 31 02 30 00".to_owned(), "44: 31 02 30 00".to_owned()]));
}

/// Remembers what it was asked to do.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<&'static str>>>);

impl Driver for Events {
    fn begin(&mut self) -> fernlicht::Result<()> {
        self.0.lock().unwrap().push("begin");
        Ok(())
    }
    fn frame(&mut self, _: &Lights, _: Duration) -> fernlicht::Result<()> {
        self.0.lock().unwrap().push("frame");
        Ok(())
    }
    fn release(&mut self) -> fernlicht::Result<()> {
        self.0.lock().unwrap().push("release");
        Ok(())
    }
}

#[test]
fn stop_ends_a_looping_show_and_releases() {
    let events = Events::default();
    let player = Player::new();
    let stop = player.stop_handle();
    let show = Show { steps: vec![Step::new(Levels::all(255), 5)], ..quick(Via::Fem, true) };

    let playing = {
        let mut driver = events.clone();
        thread::spawn(move || player.play(&show, &mut driver))
    };
    thread::sleep(Duration::from_millis(30));
    stop.stop();
    playing.join().unwrap().unwrap();

    let events = events.0.lock().unwrap();
    assert_eq!(events.first(), Some(&"begin"));
    assert_eq!(events.last(), Some(&"release"));
    assert!(events.iter().filter(|e| **e == "frame").count() > 1);
}

#[test]
fn stopped_player_does_not_start() {
    let events = Events::default();
    let player = Player::new();
    player.stop_handle().stop();
    player.play(&quick(Via::Fem, false), &mut events.clone()).unwrap();
    assert!(events.0.lock().unwrap().is_empty());
}

#[test]
fn looping_show_without_holds_is_rejected() {
    let show = Show { steps: vec![Step::new(Levels::all(255), 0)], ..quick(Via::Fem, true) };
    let err = Player::new().play(&show, &mut Events::default()).unwrap_err();
    assert!(matches!(err, Error::InvalidShow(_)));
}

#[cfg(feature = "serde")]
mod spec {
    use fernlicht::show::{Show, ShowSpec};

    fn parse(json: &str) -> fernlicht::Result<Show> {
        let spec: ShowSpec =
            serde_json::from_str(json).map_err(|e| fernlicht::Error::InvalidShow(e.to_string()))?;
        Show::try_from(spec)
    }

    fn with(field: &str, value: &str) -> String {
        let mut doc = serde_json::json!({
            "id": "mine", "name": "Mine", "via": "fle", "loop": true,
            "steps": [{ "levels": { "fl_drl": 255 }, "holdMs": 100 }],
        });
        doc[field] = serde_json::from_str(value).unwrap();
        doc.to_string()
    }

    #[test]
    fn accepts_a_valid_show() {
        let show = parse(&with("id", "\"mine\"")).unwrap();
        assert_eq!(show.steps.len(), 1);
        assert!(show.repeat);
    }

    #[test]
    fn rejects_junk() {
        assert!(parse(&with("via", "\"rem\"")).is_err());
        assert!(parse(&with("id", "\"no spaces\"")).is_err());
        assert!(parse(&with("steps", "[]")).is_err());
        assert!(parse(&with("steps", r#"[{ "levels": { "horn": 255 }, "holdMs": 100 }]"#)).is_err());
        assert!(parse(&with("steps", r#"[{ "levels": { "fl_drl": 256 }, "holdMs": 100 }]"#)).is_err());
        assert!(parse(&with("steps", r#"[{ "levels": {}, "holdMs": 1 }]"#)).is_err());
        assert!(parse(&with("steps", r#"[{ "levels": { "rl_tail": 255 }, "holdMs": 100 }]"#)).is_err());
    }
}
