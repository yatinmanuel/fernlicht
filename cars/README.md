# Cars

What is known per car. Rows come from
[car reports](https://github.com/yatinmanuel/fernlicht/issues/new?template=car-report.yml),
which are produced by the read-only `fernlicht scan`.

| Car | Headlights | Lighting modules | fem shows | fle shows |
| --- | --- | --- | --- | --- |
| F82 M4 LCI | LED | FEM_20, FLE02_L, FLE02_R, REM_20 | yes | yes |

## Most wanted

- **G-series** (G20, G30, G05, G80, …). The body controller is a BDC and its
  lamp commands are unknown. A `--full` report shows which modules exist and
  how they react to the F-series commands.
- **F-series with halogen or xenon lights**, to confirm the FEM path without
  FLE modules.
- **Other FEM cars** (F20, F22, F30, F32 and relatives), LCI or not.
- **Older F-series with an FRM** instead of a FEM (F01, F10, F25). Probably
  different commands.

## Reading a report

```json
{
  "address": 64,
  "name": "FEM_20",
  "ids": { "f150": "…", "f189": "…", "f191": "…", "f197": "…" },
  "lights": { "lampFunction": "…", "lampOutput": "…", "ledRoutine": "nrc 31" }
}
```

`ids` holds the raw identification reads: SGBD index, software version,
hardware number and name.

`lights` records how the module answered the three read-only probes described
in [PROTOCOL.md](../PROTOCOL.md#probing-an-unknown-car). `nrc 31` and `nrc 11`
mean "unknown". Any other answer makes the module a candidate worth a closer
look.
