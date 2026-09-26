mod render;

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, Subcommand};
use fernlicht::bmw::{self, Guard, Lamp, Module, ScanOptions};
use fernlicht::bytes::{Hex, parse_hex};
use fernlicht::show::{self, Player, Show, StopHandle};
use fernlicht::transport::{self, Client, ClientOptions, DEFAULT_ORDER, Direction, Framing, UdsLink};
use fernlicht::uds::{Reply, sid};

const REPORT_ISSUE: &str = "https://github.com/yatinmanuel/fernlicht/issues/new?template=car-report.yml";
const DISCOVER_TIMEOUT: Duration = Duration::from_millis(2500);
const RAW_TIMEOUT: Duration = Duration::from_secs(5);

/// Services `raw` sends without `--write`: they only read.
const READ_ONLY: [u8; 3] = [sid::READ_DID, sid::READ_DTC, sid::TESTER_PRESENT];

#[derive(Parser)]
#[command(version, about = "Drive the exterior lights of a BMW over ENET", max_term_width = 100)]
struct Cli {
    /// Try DoIP before HSFZ
    #[arg(long, global = true)]
    doip: bool,
    /// Print every request and reply on stderr
    #[arg(long, global = true)]
    trace: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Look for a car on the network (DoIP broadcast)
    Find,
    /// Identify the car and check whether its lights can be driven. Read-only
    Scan {
        host: String,
        /// Walk every address instead of the known lighting modules (takes minutes)
        #[arg(long)]
        full: bool,
        /// Where to write the report
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        /// Leave the VIN unmasked in the report
        #[arg(long)]
        keep_vin: bool,
    },
    /// List the built-in shows
    Shows,
    /// Play a show in the terminal, no car needed
    Preview {
        show: String,
        #[arg(long, default_value_t = 1.0, value_parser = parse_speed)]
        speed: f64,
    },
    /// Play a show on the car. Ctrl+C hands the lamps back
    Play {
        host: String,
        show: String,
        #[arg(long, default_value_t = 1.0, value_parser = parse_speed)]
        speed: f64,
    },
    /// Light one lamp function for a while
    Lamp {
        host: String,
        #[arg(value_parser = lamp_parser())]
        lamp: Lamp,
        /// How long, in milliseconds
        #[arg(default_value_t = 1000)]
        ms: u64,
    },
    /// Send one UDS request, e.g. `raw 169.254.92.38 40 22 f1 90`
    Raw {
        host: String,
        /// Module address in hex
        #[arg(value_parser = parse_address)]
        ecu: u16,
        /// Request bytes in hex
        #[arg(required = true, num_args = 1..)]
        request: Vec<String>,
        /// Allow services that can change state on the car
        #[arg(long)]
        write: bool,
    },
}

fn parse_speed(s: &str) -> Result<f64, String> {
    let speed: f64 = s.parse().map_err(|_| format!("{s:?} is not a number"))?;
    if (0.25..=4.0).contains(&speed) { Ok(speed) } else { Err("speed must be between 0.25 and 4".into()) }
}

fn parse_address(s: &str) -> Result<u16, String> {
    u16::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|_| format!("{s:?} is not a hex address"))
}

fn lamp_parser() -> impl TypedValueParser<Value = Lamp> {
    PossibleValuesParser::new(Lamp::ALL.map(Lamp::name))
        .map(|name: String| name.parse::<Lamp>().expect("listed name"))
}

struct Session {
    framings: Vec<Framing>,
    options: ClientOptions,
}

impl Session {
    fn new(cli: &Cli) -> Self {
        let mut framings = DEFAULT_ORDER.to_vec();
        if cli.doip {
            framings.reverse();
        }
        let mut options = ClientOptions::default();
        if cli.trace {
            options.trace = Some(Arc::new(|direction, ecu, uds: &[u8]| {
                let arrow = if direction == Direction::Sent { '>' } else { '<' };
                eprintln!("{arrow} {ecu:02x} {}", Hex(uds));
            }));
        }
        Self { framings, options }
    }

    fn connect(&self, host: &str) -> fernlicht::Result<Client> {
        transport::connect(host, &self.framings, &self.options)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let session = Session::new(&cli);
    let result = match cli.command {
        Command::Find => find(),
        Command::Scan { host, full, out, keep_vin } => {
            scan(&session, &host, ScanOptions { full, lights: true, keep_vin }, out)
        }
        Command::Shows => {
            list_shows();
            Ok(())
        }
        Command::Preview { show, speed } => preview(&show, speed),
        Command::Play { host, show, speed } => play(&session, &host, &show, speed),
        Command::Lamp { host, lamp, ms } => light(&session, &host, lamp, Duration::from_millis(ms)),
        Command::Raw { host, ecu, request, write } => raw(&session, &host, ecu, &request.join(" "), write),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn find() -> anyhow::Result<()> {
    let hosts = transport::discover(DISCOVER_TIMEOUT).context("broadcast failed")?;
    if hosts.is_empty() {
        println!("nothing answered");
    }
    for host in hosts {
        println!("{host}");
    }
    Ok(())
}

fn print_module(module: &Module) {
    let name = module.name.as_deref().unwrap_or("?");
    let known = module.lights.as_ref().map(bmw::LightProbe::recognised).unwrap_or_default();
    let hint = if known.is_empty() { String::new() } else { format!("  <- knows {}", known.join(", ")) };
    let line = format!("  {:#04x}  {name:<12}{hint}", module.address);
    println!("{}", line.trim_end());
}

fn scan(session: &Session, host: &str, options: ScanOptions, out: Option<PathBuf>) -> anyhow::Result<()> {
    let client = session.connect(host)?;
    if options.full {
        println!("walking every address, this takes a few minutes");
    }
    let report = bmw::identify(&client, options, print_module);
    let profile = report.profile;

    println!();
    println!("vin        {}", report.vin.as_deref().unwrap_or("not readable"));
    println!("transport  {}", report.framing);
    if let Some(volts) = profile.body.and_then(|body| bmw::read_voltage(&client, body)) {
        println!("battery    {volts:.1} V");
    }
    let has_body = profile.body.is_some();
    let has_headlights = profile.left.is_some() && profile.right.is_some();
    println!("fem shows  {}", if has_body { "yes" } else { "no, no FEM_20 found" });
    println!("fle shows  {}", if has_headlights { "yes" } else { "no, needs FLE02_L and FLE02_R" });

    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let path = out.unwrap_or_else(|| PathBuf::from(format!("fernlicht-report-{millis}.json")));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    writeln!(file)?;

    println!("\nwrote {}", path.display());
    if !has_body || !has_headlights {
        println!("Parts of this car are not mapped yet. Attaching the report here helps map them:");
        println!("{REPORT_ISSUE}");
    }
    Ok(())
}

fn list_shows() {
    for show in show::builtin() {
        let mode = if show.repeat { "loop" } else { "once" };
        println!("{:<14} {}  {mode}  {}", show.id, show.via.name(), show.name);
    }
}

fn find_show(id: &str) -> anyhow::Result<Show> {
    show::find(id).with_context(|| format!("no show called {id:?}, see `fernlicht shows`"))
}

/// First Ctrl+C stops the show and restores the lamps, a second one quits at once.
fn stop_on_interrupt(stop: StopHandle) -> anyhow::Result<()> {
    let interrupted = AtomicBool::new(false);
    ctrlc::set_handler(move || {
        if interrupted.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
        stop.stop();
    })
    .context("cannot install Ctrl+C handler")
}

fn preview(id: &str, speed: f64) -> anyhow::Result<()> {
    let show = find_show(id)?;
    let player = Player::new().with_speed(speed);
    stop_on_interrupt(player.stop_handle())?;
    let mut preview = render::Preview::new(io::stdout().lock())?;
    player.play(&show, &mut preview)?;
    Ok(())
}

fn play(session: &Session, host: &str, id: &str, speed: f64) -> anyhow::Result<()> {
    let show = find_show(id)?;
    let client = session.connect(host)?;
    let profile = bmw::identify(&client, ScanOptions::default(), |_| {}).profile;
    let link = Guard::new(&client, profile);
    let mut driver = show::driver_for(&show, &link, &profile)?;

    let player = Player::new().with_speed(speed);
    stop_on_interrupt(player.stop_handle())?;
    println!("{}{}", show.name, if show.repeat { ", Ctrl+C to stop" } else { "" });
    player.play(&show, &mut driver)?;
    Ok(())
}

fn light(session: &Session, host: &str, lamp: Lamp, duration: Duration) -> anyhow::Result<()> {
    let client = session.connect(host)?;
    let profile = bmw::identify(&client, ScanOptions::default(), |_| {}).profile;
    let Some(body) = profile.body else {
        bail!("no FEM_20 on this car");
    };
    Guard::new(&client, profile).send(body, &bmw::fem_lamp(lamp, duration))?;
    Ok(())
}

fn raw(session: &Session, host: &str, ecu: u16, hex: &str, write: bool) -> anyhow::Result<()> {
    let request = parse_hex(hex)?;
    if !write && !READ_ONLY.contains(&request[0]) {
        bail!("service {:#04x} can change state on the car, pass --write if you mean it", request[0]);
    }
    let client = session.connect(host)?;
    match client.request(ecu, &request, RAW_TIMEOUT)? {
        Reply::Positive { sid, data } => println!("{:02x} {}", sid + 0x40, Hex(&data)),
        Reply::Negative { nrc, .. } => println!("negative: {nrc}"),
    }
    Ok(())
}
