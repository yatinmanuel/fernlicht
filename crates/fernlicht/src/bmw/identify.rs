//! Read-only identification: which modules the car has, what they are called,
//! and whether they recognise the light commands.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, SystemTime};

use super::tables::{FLE_ROUTINE, did, ecu};
use crate::bytes::{Hex, be_u16};
use crate::transport::{Client, Framing, UdsLink};
use crate::uds::{self, Nrc, Reply, Routine};

const PING_TIMEOUT: Duration = Duration::from_millis(350);
const READ_TIMEOUT: Duration = Duration::from_millis(600);
const VIN_TIMEOUT: Duration = Duration::from_millis(1000);
const VOLTAGE_TIMEOUT: Duration = Duration::from_millis(1500);

const IDENTIFICATION: [u16; 4] = [did::ECU_NAME, did::SGBD_INDEX, did::HW_NUMBER, did::SW_VERSION];

/// How a module reacted to one read-only request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Positive reply, echoed identifier stripped.
    Data(Vec<u8>),
    Negative(Nrc),
    NoAnswer,
}

impl Answer {
    /// Whether the module appears to know the identifier: anything except
    /// silence, "request out of range" and "service not supported".
    ///
    /// Only a hint. A write-only identifier answers a read with "out of
    /// range" even though writing it works.
    pub fn recognised(&self) -> bool {
        !matches!(
            self,
            Answer::NoAnswer | Answer::Negative(Nrc::REQUEST_OUT_OF_RANGE | Nrc::SERVICE_NOT_SUPPORTED)
        )
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Answer::Data(data) => Hex(data).fmt(f),
            Answer::Negative(nrc) => write!(f, "nrc {:02x}", nrc.0),
            Answer::NoAnswer => f.write_str("no answer"),
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Answer {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// How a module answered questions about the three light commands.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize), serde(rename_all = "camelCase"))]
pub struct LightProbe {
    pub lamp_function: Answer,
    pub lamp_output: Answer,
    pub led_routine: Answer,
}

impl LightProbe {
    /// Names of the commands the module seems to recognise.
    pub fn recognised(&self) -> Vec<&'static str> {
        [
            ("lamp function", &self.lamp_function),
            ("lamp output", &self.lamp_output),
            ("led routine", &self.led_routine),
        ]
        .into_iter()
        .filter(|(_, answer)| answer.recognised())
        .map(|(name, _)| name)
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Module {
    pub address: u16,
    pub name: Option<String>,
    /// Identification reads keyed by DID in hex.
    pub ids: BTreeMap<String, Answer>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub lights: Option<LightProbe>,
}

/// Where the lighting modules of one particular car live. `None` means the
/// module was not found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Profile {
    pub body: Option<u16>,
    pub left: Option<u16>,
    pub right: Option<u16>,
    pub rear: Option<u16>,
}

/// Everything a scan found. Serialises to the report file people attach to a
/// car report.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize), serde(rename_all = "camelCase"))]
pub struct Report {
    pub schema: u32,
    #[cfg_attr(feature = "serde", serde(serialize_with = "crate::time::serialize_rfc3339"))]
    pub created_at: SystemTime,
    pub framing: Framing,
    /// Serial number masked unless asked otherwise.
    pub vin: Option<String>,
    pub modules: Vec<Module>,
    pub profile: Profile,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions {
    /// Walk every address from 0x01 to 0xfe instead of the known lighting
    /// modules. Takes minutes.
    pub full: bool,
    /// Also ask each module about the light commands. Still read-only.
    pub lights: bool,
    /// Keep the full VIN in the report.
    pub keep_vin: bool,
}

fn printable(bytes: &[u8]) -> String {
    let text: String = bytes.iter().filter(|b| (0x20..0x7f).contains(*b)).map(|&b| char::from(b)).collect();
    text.trim().to_owned()
}

/// Extracts the VIN from the data of a positive `22 f1 90` reply.
pub fn parse_vin(data: &[u8]) -> Option<String> {
    let text = printable(data.get(2..)?);
    let vin = text.get(text.len().checked_sub(17)?..)?;
    let valid = vin.bytes().all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"IOQ".contains(&b)));
    valid.then(|| vin.to_owned())
}

/// Keeps manufacturer, model, year and plant; hides the serial number.
pub fn mask_vin(vin: &str) -> String {
    let keep: String = vin.chars().take(11).collect();
    format!("{keep}******")
}

/// Reads the VIN from the gateway, falling back to the FEM.
pub fn read_vin(link: &impl UdsLink) -> Option<String> {
    [ecu::GATEWAY, ecu::FEM].into_iter().find_map(|address| {
        let reply = link.request(address, &uds::read_did(did::VIN), VIN_TIMEOUT).ok()?;
        parse_vin(reply.data()?)
    })
}

/// Battery voltage as the module at `address` (normally the FEM) measures it.
pub fn read_voltage(link: &impl UdsLink, address: u16) -> Option<f64> {
    let reply = link.request(address, &uds::read_did(did::VOLTAGE), VOLTAGE_TIMEOUT).ok()?;
    let volts = f64::from(be_u16(reply.data()?, 2)?) / 10.0;
    (volts > 5.0 && volts < 18.0).then_some(volts)
}

fn ask(link: &impl UdsLink, address: u16, request: &[u8], echo: usize) -> Answer {
    match link.request(address, request, READ_TIMEOUT) {
        Ok(Reply::Positive { data, .. }) => Answer::Data(data.get(echo..).unwrap_or_default().to_vec()),
        Ok(Reply::Negative { nrc, .. }) => Answer::Negative(nrc),
        Err(_) => Answer::NoAnswer,
    }
}

/// Asks a module about the light commands without running any of them: reads
/// of the two DIDs and a results query for the LED routine.
pub fn probe_lights(link: &impl UdsLink, address: u16) -> LightProbe {
    LightProbe {
        lamp_function: ask(link, address, &uds::read_did(did::LAMP_FUNCTION), 2),
        lamp_output: ask(link, address, &uds::read_did(did::LAMP_OUTPUT), 2),
        led_routine: ask(link, address, &uds::routine(Routine::Results, FLE_ROUTINE, &[]), 3),
    }
}

/// Finds modules and reads their identification. Only ever reads.
///
/// `on_module` is called as each module is found, for progress output.
pub fn scan(link: &impl UdsLink, options: ScanOptions, mut on_module: impl FnMut(&Module)) -> Vec<Module> {
    let addresses: Vec<u16> = if options.full { (0x01..=0xfe).collect() } else { ecu::LIGHTING.to_vec() };
    let mut modules = Vec::new();

    for address in addresses {
        // Any reply counts, a negative one still proves something lives here.
        if link.request(address, &uds::tester_present(false), PING_TIMEOUT).is_err() {
            continue;
        }
        let mut module = Module { address, name: None, ids: BTreeMap::new(), lights: None };
        for id in IDENTIFICATION {
            let answer = ask(link, address, &uds::read_did(id), 2);
            if let (did::ECU_NAME, Answer::Data(data)) = (id, &answer) {
                module.name = Some(printable(data)).filter(|name| !name.is_empty());
            }
            module.ids.insert(format!("{id:04x}"), answer);
        }
        if options.lights {
            module.lights = Some(probe_lights(link, address));
        }
        on_module(&module);
        modules.push(module);
    }
    modules
}

/// Picks out the lighting modules by the name they report.
///
/// The address alone proves nothing: a G-series BDC answers at 0x40 like a
/// FEM does, and speaks a different command set.
pub fn resolve_profile(modules: &[Module]) -> Profile {
    let find = |wanted: &str| {
        modules
            .iter()
            .find(|m| m.name.as_deref().is_some_and(|name| normalise(name) == wanted))
            .map(|m| m.address)
    };
    Profile { body: find("FEM_20"), left: find("FLE02_L"), right: find("FLE02_R"), rear: find("REM_20") }
}

fn normalise(name: &str) -> String {
    name.chars().map(|c| if matches!(c, ' ' | '.' | '-') { '_' } else { c.to_ascii_uppercase() }).collect()
}

/// Reads the VIN, scans, and works out the profile. Read-only.
pub fn identify(client: &Client, options: ScanOptions, on_module: impl FnMut(&Module)) -> Report {
    let vin = read_vin(client);
    let modules = scan(client, options, on_module);
    Report {
        schema: 1,
        created_at: SystemTime::now(),
        framing: client.framing(),
        vin: vin.map(|vin| if options.keep_vin { vin } else { mask_vin(&vin) }),
        profile: resolve_profile(&modules),
        modules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vin_from_reply_data() {
        let mut data = vec![0xf1, 0x90];
        data.extend_from_slice(b"WBA00000000000042");
        assert_eq!(parse_vin(&data).as_deref(), Some("WBA00000000000042"));
        assert_eq!(parse_vin(&[0xf1, 0x90, b'S', b'H', b'O', b'R', b'T']), None);
        assert_eq!(parse_vin(b"\xf1\x90WBAI0000000000042"), None);
    }

    #[test]
    fn masked_vin_keeps_the_model() {
        assert_eq!(mask_vin("WBA00000000000042"), "WBA00000000******");
    }

    #[test]
    fn profile_goes_by_name() {
        let module = |address, name: &str| Module {
            address,
            name: Some(name.into()),
            ids: BTreeMap::new(),
            lights: None,
        };
        let profile =
            resolve_profile(&[module(0x40, "BDC_BODY"), module(0x51, "fle02-l"), module(0x52, "FLE02_R")]);
        assert_eq!(profile, Profile { body: None, left: Some(0x51), right: Some(0x52), rear: None });
    }

    #[test]
    fn recognised_answers() {
        assert!(!Answer::Negative(Nrc::REQUEST_OUT_OF_RANGE).recognised());
        assert!(!Answer::NoAnswer.recognised());
        assert!(Answer::Negative(Nrc::CONDITIONS_NOT_CORRECT).recognised());
        assert_eq!(Answer::Negative(Nrc(0x31)).to_string(), "nrc 31");
    }
}
