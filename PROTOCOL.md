# Protocol

This is the byte-level reference for everything fernlicht sends. Where it says
*verified*, the behaviour was confirmed on an F82 M4 LCI. Everything else comes
from the standards and from BMW's ECU description files.

## Physical link

F- and G-series cars carry 100BASE-TX on OBD pins 3/11 and 12/13, and pin 8
switches it on. An ENET cable is only a wiring adapter; the Wi-Fi dongles put a
small access point in front of the same connection. On the other side sits the
central gateway (ZGW). It routes diagnostic requests to the rest of the car by
module address.

## Framing

The gateway speaks one of two framings. F-series use HSFZ, G-series DoIP. The
connection code tries HSFZ first, because a closed port fails at once while an
unanswered DoIP activation has to wait for its timeout.

### HSFZ (TCP 6801)

```text
00 00 00 05 | 00 01 | f4 | 40 | 22 f1 90
length        kind    src  dst  UDS
```

| Field | Size | Meaning |
| --- | --- | --- |
| length | 4 | bytes after `kind` (addresses + UDS) |
| kind | 2 | `0001` diagnostic, `0002` acknowledgement, `0012` alive check, `0040`–`0045` and `00ff` errors |
| src, dst | 1 each | tester is `f4` |

The gateway sends each request back as a `0002` acknowledgement before the real
`0001` answer arrives. There is no handshake.

| Error kind | Meaning |
| --- | --- |
| `0040` | incorrect tester address |
| `0041` | incorrect control word |
| `0042` | incorrect format |
| `0043` | incorrect destination address |
| `0044` | message too large |
| `0045` | diagnostic application not ready |

### DoIP (TCP 13400, ISO 13400-2)

```text
02 fd | 80 01 | 00 00 00 07 | 0e f8 | 00 40 | 22 f1 90
ver     type    length        src     dst     UDS
```

The tester address is `0ef8`. Before the first diagnostic message, activate
routing and wait for response code `10`:

```text
> 02 fd 00 05 00 00 00 07  0e f8 00 00 00 00 00
< 02 fd 00 06 00 00 00 09  0e f8 00 10 10 00 00 00 00
                                       ^^ accepted
```

| Type | Direction | Meaning |
| --- | --- | --- |
| `0001` | → UDP broadcast | vehicle identification request (used by `find`) |
| `0005` / `0006` | → / ← | routing activation request / response |
| `0007` / `0008` | ← / → | alive check / reply with our address |
| `8001` | ↔ | diagnostic message |
| `8002` / `8003` | ← | diagnostic acknowledgement / negative acknowledgement |

## Sessions and timing

Forced outputs last only as long as a non-default diagnostic session, and the
module drops the session a few seconds after its last request. The client sends
`3e 80` (tester present, no reply) every two seconds while idle.

Every module shares the one gateway connection, and replies can be late. A
reply counts as the answer only if the source address, the service and the
echoed identifier all match. After a timeout, the same request is not repeated
on that connection: a late answer to the first attempt would look exactly like
the answer to the second.

## Modules

| Address | Name | Role | |
| --- | --- | --- | --- |
| `10` | ZGW_01 | central gateway | verified |
| `40` | FEM_20 | front electronics, owns the exterior lighting | verified |
| `43` | FLE02_L | left LED headlight | verified |
| `44` | FLE02_R | right LED headlight | verified |
| `60` | KOMBI | instrument cluster | |
| `72` | REM_20 | rear electronics | verified |

| DID | Content |
| --- | --- |
| `f197` | module name, ASCII |
| `f190` | VIN, ASCII |
| `f150` | SGBD index: which description file applies |
| `f191` | hardware number |
| `f189` | software version |
| `dad6` | terminal 30 voltage on the FEM, 2 bytes, 0.1 V |

A G-series BDC also answers at `40`, with a different command set. Identify
modules by name, never by address alone.

## Light commands

### FEM lamp function: `2e d5 42`

```text
> 40  2e d5 42  00 03  00 14      low beam for 200 ms
< 40  6e d5 42
```

This is a write to DID `d542` (`LEUCHTEN_FUNKTION`). The data is a 2-byte lamp
function and a 2-byte duration in 10 ms ticks. Both fields are required at full
width; anything shorter gets NRC `13` (verified). The FEM switches the lamp off
by itself when the time runs out. A show resends the function each step, with
a duration a little longer than the step so the lamp does not flicker.

| Code | Function | Code | Function |
| --- | --- | --- | --- |
| `01` | position | `0a` | cornering right |
| `03` | low beam | `0c` | brake |
| `04` | daytime running | `0e` | rear fog |
| `05` | high beam | `0f` | reverse |
| `06` | indicators left | `10` | parking left |
| `07` | indicators right | `11` | parking right |
| `08` | front fog | `12` | hazards |
| `09` | cornering left | `13` | interior |

Functions act on the whole car: `05` lights both high beams, `06` every left
indicator front and rear. A function can switch lamps on, not off. Lamp `00`
with time `0` ends everything early.

### FEM single output: `2f 45 01`

```text
> 40  2f 45 01  03  fe 00        force every output off
< 40  6f 45 01  03
> 40  2f 45 01  00  fe           return control
```

This is standard I/O control on DID `4501` (`STEUERN_LEUCHTENAUSGANG_DIGITAL`).
`03` means short-term adjustment, followed by the output and the state; `00`
returns control. The control parameter is required (NRC `13` without it,
verified). The description file mentions action values `40`/`80`, which the car
rejects.

Outputs: `01`/`02` low beam L/R, `03`/`04` DRL, `05`/`06` side, `07`/`08` high
beam, `09`/`0a` position, `0b`/`0c` fog, `12`/`13` bi-xenon shutter, `30`/`31`
rings, `fe` all.

With LED headlights the FLE modules drive the front lamps. The FEM accepts
the command but nothing visible changes (verified: "all off" only turned off the
interior and footwell lights).

### FLE LED channels: `31 01 30 00`

```text
> 43  31 01 30 00  32 64  32 64  …   ten (current, pwm) pairs
< 43  71 01 30 00
> 43  31 02 30 00                    stop, back to normal
```

This starts routine `3000` (`_LEUCHTEN_AUSSENLICHT_KANAL`). The data is ten
channels of one current byte and one PWM byte each, PWM from 0 to 100. A
current of `32` works and `ff` gets NRC `31` (verified); the real ceiling has
not been searched for, so fernlicht never goes above `32`. Which channel feeds
which LED group is not known yet, so every channel is driven alike.

### REM rear outputs: `2e 45 01`

```text
> 72  2e 45 01  22 00            number plate light off
< 72  6e 45 01
```

This uses the same DID as the FEM output, but as a plain write with no control
parameter. Outputs: `14`/`15` tail L/R, `16`/`17` second tail, `18`/`19` brake,
`1a`/`1b` brake force display, `1c`/`1d` rear fog, `1e`/`1f` reverse, `20`/`21`
indicators, `22` plate, `23` centre brake.

There is no return control, and a forced-off output does not reliably recover
when the session ends. To restore it, write it back on (`… 22 01`) and send
`10 01`. The built-in shows reach the rear through FEM lamp functions instead.

## Probing an unknown car

These requests only read, and they show whether a module knows a command:

| Request | Asks |
| --- | --- |
| `22 f1 97` | name |
| `22 f1 50` | SGBD index |
| `22 d5 42` | is the lamp function DID known? |
| `22 45 01` | is the lamp output DID known? |
| `31 03 30 00` | results of routine `3000`: is the LED routine known? |

NRC `31` means the identifier is unknown and `11` that the service is. Any
other answer, positive or negative, suggests the identifier exists. This is a
hint, not proof: a write-only DID can answer `31` to a read. `fernlicht scan`
records these answers in its report.

## Negative responses

| NRC | Usual cause here |
| --- | --- |
| `11` | wrong module for this service |
| `13` | wrong length: missing time, missing control parameter, 1-byte field |
| `22` | module refuses in its current state |
| `31` | out of range: FLE current too high, unknown lamp or output |
| `78` | response pending; keep waiting |
| `7e` / `7f` | not in this session; send `10 03` first |

## References

- ISO 14229-1, Unified Diagnostic Services
- ISO 13400-2, Diagnostic communication over Internet Protocol
- [EdiabasLib](https://github.com/uholeschak/ediabaslib), an open
  implementation of BMW's diagnostic runtime, including HSFZ
- BMW's ECU description files (`*.prg`) name the jobs, DIDs and tables. They
  are BMW's property and are not part of this repository.
