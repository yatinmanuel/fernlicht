<p align="center">
  <picture>
    <source srcset="docs/logo-dark.svg" media="(prefers-color-scheme: dark)">
    <img src="docs/logo-light.svg" alt="fernlicht" width="420">
  </picture>
</p>

<p align="center">Drive the exterior lights of a BMW from Rust, over an ENET cable.</p>

---

fernlicht connects to the diagnostic gateway of an F-series BMW, talks UDS to
the lighting modules behind it, and plays light shows on the car: flashes,
chases, and on LED headlights, smooth fades.

It is a library and a command line tool. The library has no required
dependencies and runs over any byte stream, so the same code works with a TCP
socket, a test double or whatever your platform provides.

## Will it work on my car?

Connect an ENET cable and run a scan. It only reads: module names,
part numbers, and whether each module knows the light commands.

```console
$ fernlicht scan 169.254.92.38
  0x10  ZGW_01
  0x40  FEM_20
  0x43  FLE02_L
  0x44  FLE02_R
  0x72  REM_20

vin        WBS00000000******
transport  hsfz
battery    12.4 V
fem shows  yes
fle shows  yes

wrote fernlicht-report-1790000000000.json
```

| Car | Status |
| --- | --- |
| F-series with FEM_20, FLE02 LED headlights and REM_20 | Supported |
| F-series with halogen or xenon headlights | `fem` shows only; there is no FLE to dim |
| G-series (BDC body controller) | Connects over DoIP and scans. No light commands yet |
| E-series | Not supported. Different bus (K-line / D-CAN) |

If the scan says "no", the report file is still useful. It lists what the car
has instead, and a module that seems to recognise a light command is marked
`<- knows …`. [Open a car report](https://github.com/yatinmanuel/fernlicht/issues/new?template=car-report.yml)
with the file attached; the serial part of the VIN is masked. `--full` walks
all 254 addresses and takes a few minutes, which is worth it on anything that
is not an F3x/F8x. [cars/](cars/README.md) tracks what has been seen so far.

## Install

```sh
cargo install --git https://github.com/yatinmanuel/fernlicht fernlicht-cli
```

You need an ENET cable or ENET Wi-Fi adapter in the OBD port, and your machine
on the same network. `fernlicht find` broadcasts for the gateway. If nothing
answers, a cable usually sits on `169.254.x.x` and a Wi-Fi adapter on the
address its DHCP server hands out.

## Command line

```text
fernlicht find                        look for a car on the network
fernlicht scan <host> [--full]        identify the car, write a report (read-only)
fernlicht shows                       list the built-in shows
fernlicht preview <show>              play a show in the terminal, no car needed
fernlicht play <host> <show>          play it on the car; Ctrl+C hands the lamps back
fernlicht lamp <host> low-beam 500    one lamp function for 500 ms
fernlicht raw <host> 40 22 f1 90      one UDS request; only reads without --write
```

`--trace` prints every request and reply, `--doip` tries DoIP first.

## Library

```toml
[dependencies]
fernlicht = { git = "https://github.com/yatinmanuel/fernlicht" }
```

```rust
use fernlicht::bmw::{self, Guard, ScanOptions};
use fernlicht::show::{self, Player};
use fernlicht::transport::{self, ClientOptions};

let client = transport::connect("169.254.92.38", &transport::DEFAULT_ORDER, &ClientOptions::default())?;
let report = bmw::identify(&client, ScanOptions::default(), |_| {});  // read-only
let link = Guard::new(&client, report.profile);                       // only light commands pass

let welcome = show::find("welcome").unwrap();
let mut driver = show::driver_for(&welcome, &link, &report.profile)?;
Player::new().play(&welcome, &mut driver)?;
```

Underneath, commands are plain functions that return request bytes:

```rust
use std::time::Duration;
use fernlicht::bmw::{Lamp, fem_lamp, fle_leds};
use fernlicht::transport::UdsLink;

link.send(0x40, &fem_lamp(Lamp::HighBeam, Duration::from_millis(300)))?; // 2e d5 42 00 05 00 1e
link.send(0x43, &fle_leds(35))?;                                        // left headlight at 35 %
```

A show is a list of steps. Levels go from 0 to 255, and outputs a step leaves
out keep their level. With the `serde` feature, shows load from JSON and are
validated on the way in:

```json
{
  "id": "mine", "name": "Mine", "via": "fle", "loop": true,
  "steps": [
    { "levels": { "fl_drl": 255, "fr_drl": 0 }, "holdMs": 200 },
    { "levels": { "fl_drl": 0, "fr_drl": 255 }, "holdMs": 200 }
  ]
}
```

`via: "fem"` uses the front module's lamp functions: on/off only, but it
reaches every lamp, rear included. `via: "fle"` dims the LED headlights with
PWM, so it can fade, but only at the front.

To run over something other than TCP, implement
[`Transport`](crates/fernlicht/src/transport/mod.rs) (two methods) and pass it
to `Client::new`.

## Safety

fernlicht uses the same diagnostic services as a workshop tester. It never
codes, flashes or writes anything that survives a restart.

- **Only when parked.** You are overriding the exterior lighting.
- Stopping a show, or Ctrl+C in the CLI, hands the lamps back and reports any
  module that did not confirm. If the connection drops mid-show, the modules
  fall back when their diagnostic session times out. If a headlight stays in an
  odd state, cycle the ignition.
- The REM output command is in the library but no built-in show uses it. It
  has no release; see [PROTOCOL.md](PROTOCOL.md#rem-rear-outputs).
- LED shows draw current with the engine off. `scan` shows the battery voltage.
- `Guard` rejects everything except reads and the known light commands, and
  sends those only to modules that identified themselves. Keep user input
  behind it.

MIT licensed, no warranty. It is your car.

## Development

```sh
cargo test --workspace --all-features   # runs against an in-memory gateway
cargo clippy --workspace --all-targets --all-features
cargo xtask logo                        # redraw the logo from brand/*.txt
```

[PROTOCOL.md](PROTOCOL.md) documents the bytes on the wire.
