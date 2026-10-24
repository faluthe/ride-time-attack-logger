# Ride Time Attack: logger

Live telemetry and data tools for **Ride 4** on Linux (Steam/Proton). The logger reads the
player's bike and the game's lap timing straight out of memory at 50 Hz, shows them on a Dear
ImGui dashboard and records sessions to CSV. `rta-export` turns sessions, your save games and
the game's own files into the data the site shows.

```sh
cargo run --release --bin rta-logger                 # dashboard; records while you're on track
cargo run --release --bin rta-export -- --out ../site/public/data
cargo run --release --bin rta-export -- --out ../site/public/data --upload   # and send it to R2
cargo run --release -p ride4-mem --example probe      # text readout, handy after a game update
```

`rta-logger` options: `--sessions DIR`, `--no-record`, `--light`, and
`--screenshot FILE.png [--shot-delay S]`, which saves one frame and exits.

`rta-export` options: `--game DIR`, `--saves DIR`, `--sessions DIR`, `--extract`, which
re-reads tracks and events from the game paks after a game update, and `--upload` (see
"Uploading").

Data lives in `~/.local/share/ride-time-attack/`:
- `sessions/*.csv`: one file per stint on track
- `tracks.json`, `events.json`: layouts and career events extracted from the game paks (cached)

## Crates

| Crate | What it does |
|---|---|
| `ride4-mem` | `/proc/<pid>/mem` access, FNamePool lookups, the bike locator and lap timing (`src/offsets.rs`); `examples/` holds the scanners used to find offsets |
| `ride4-data` | pak (v9, zlib), cooked asset and GVAS readers; track, career-event and save extraction |
| `telemetry` | samples, CSV sessions, track projection, lap cutting |
| `export` | `rta-export` |
| `upload` | sends an export to the site's R2 bucket over the S3 API (SigV4), only what changed |
| `app` | `rta-logger`: reader thread plus the ImGui dashboard (winit + glutin + glow) |

## How the game is read

The exe loads at `0x140000000` (no ASLR), so the root is a fixed global:

```
GWorld 0x145c80d80 → +0x188 GameInstance → +0x38 LocalPlayers[0] → +0x30 PlayerController
       → +0x250 Pawn (BaseMotorBike_C; time-trial ghosts are separate actors)
GWorld → +0x130 GameState → +0x358 HudLogicComponent → +0xF8 TimedResultsManager → +0xD0 items[0]
```

| Channel | Where | Type |
|---|---|---|
| Position, rotation | bike `+0x130` mesh → quat `+0x1C0`, translation `+0x1D0` | f32, cm |
| Throttle, brake, rear | bike `+0x290` movement → `+0x1DC`, `+0x1E0`, `+0x1E4` | f32 0..1 (rear is 0/1) |
| Gear | bike `+0x8F0` IgnitionBike → `+0x264` | i32, 0 = neutral |
| Engine speed | bike `+0x1070` audio component → `+0x238` | f32, idle 0 .. limiter 1 |
| Race clock | HudLogic `+0x174` | f32 s, stops while paused |
| Best lap | HudLogic `+0xE8` | f32 s |
| Laps done, last lap | result item `+0xA0`, `+0xA8` | i32, i32 ms |

Lap timing exists in Time Attack; other modes record without it. If an update moves GWorld, the
reader rescans `.data` for a pointer whose chain ends at a `BaseMotorBike_C` (checked by class
name).

## Laps

With the game's timing, a lap ends when the completed-lap counter goes up and lasts exactly the
game's last-lap time, on the race clock (so pauses don't count). Sessions without timing fall
back to crossings of the track spline's start, about ±0.1 s off the game.

## CSV columns

`t,x,y,z,fx,fy,fz,side_z,up_z,throttle,brake,rear,gear,rpm,clock,laps,last_lap_ms`: time (s),
position (m), forward vector, z of the side and up axes (lean = −asin(side_z)), inputs, then the
game's race clock, completed laps (−1 without timing) and last lap (ms). Older 9- and 14-column
files still load.

## Exported data

`rta-export --out DIR` writes:

| File | Contents |
|---|---|
| `tracks.json` | layouts (centre line, edges) by track key |
| `leaderboards.json`, `career.json` | from the save games |
| `laps.json` | the lap index: `{ "generated": unix seconds, "laps": [ ... ] }` |
| `laps/<id>.bin.gz` | one lap's trace (see "Lap files"); stale ones are removed |

Each `laps` entry has every lap field except the trace: `id`, `session`, `lap`, `track`,
`valid`, `problem` (string or null), `time` (s), `game_timed`, `top_speed`, `avg_speed` (km/h),
`max_lean_left`, `max_lean_right` (deg), `max_brake_g`, `max_accel_g`, and when the session has
inputs `full_throttle_pct`, `braking_pct`, `top_gear`. On top of that, `samples` is the trace
length and `file` the trace's path relative to `laps.json` (`laps/<id>.bin.gz`).

There's no `data.js` bundle any more: traces are fetched per lap, which `file://` can't do
anyway, so serve the site over HTTP.

## Uploading

`rta-export --upload` makes the `ride-time-attack-data` R2 bucket match the export: lap files
and JSON that are new or changed (by MD5 against the ETag) go up first, `laps.json` last, then
lap files the index no longer lists are deleted. It refuses an export with no laps when the
bucket has some. Credentials come from the environment or a `.env` file (searched for from the
current directory up); copy `.env.example` to `.env` and fill in an R2 API token with Object
Read & Write on the bucket. The token can write the whole bucket, so it's single user for now.

## Lap files

A lap file is a **gzip stream** (RFC 1952; in the browser,
`new Response(blob.stream().pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()`) of the
binary trace below. If a server already undid the gzip (`Content-Encoding`), the bytes start
with `RTAL` instead of `1f 8b`; check the first two bytes and skip the gunzip.

The trace is little-endian throughout, at 25 samples per second, column by column:

| Offset | Type | Field |
|---|---|---|
| 0 | 4 bytes | magic `RTAL` (`52 54 41 4c`) |
| 4 | u16 | version, `1` |
| 6 | u16 | channel count `C` |
| 8 | u32 | sample count `N` (same for every channel) |
| 12 | `C` × 12 bytes | channel table |
| 12 + 12·`C` | | channel data, in table order |

Channel table entry (12 bytes):

| Offset | Type | Field |
|---|---|---|
| 0 | 4 bytes | name, ASCII, padded with `00` (`t`, `lean`, …) |
| 4 | u8 | delta width `W` in bytes: 1, 2 or 4 |
| 5 | u8 | decimals `k` |
| 6 | u16 | reserved, 0 |
| 8 | i32 | first quantised value `q[0]` |

Channel data: `N − 1` signed deltas of `W` bytes each (i8, i16 or i32), then zero padding up to a
multiple of 4 bytes, so every block starts 4-byte aligned (an `Int8Array`/`Int16Array`/
`Int32Array` view over the decompressed buffer works, as does `DataView`). The encoder picks the
narrowest `W` that holds every delta of that channel in that lap. Decoding:

```
q[0] = first;  q[i] = (q[i−1] + delta[i−1]) | 0     // i32 wrap-around, i = 1 .. N−1
value[i] = q[i] / 10^k                               // divide (don't multiply by 0.1)
```

Dividing gives exactly the numbers the old JSON traces had. `N = 0` means no data blocks;
`N = 1` means empty blocks. Skip channels you don't know; rely on names, not positions.

Channels, in the order they're written (the last five only when the session has inputs):

| Name | Unit | `k` | Step | Notes |
|---|---|---|---|---|
| `t` | s since lap start | 3 | 1 ms | race clock when the game timed the lap |
| `d` | m from the start line | 1 | 0.1 m | slightly negative just before the line |
| `x`, `y` | m, world position | 2 | 1 cm | |
| `v` | km/h | 1 | 0.1 | |
| `lean` | degrees, right positive | 1 | 0.1° | |
| `ax` | longitudinal g | 2 | 0.01 g | |
| `thr`, `brk` | 0..1 | 2 | 0.01 | |
| `rear` | 0 or 1 | 0 | | rear brake button |
| `gear` | integer, 0 = neutral | 0 | | |
| `rpm` | 0 = idle .. 1 = limiter | 3 | 0.001 | |

A 160 s lap is about 60 KB of trace before gzip and 20–25 KB after (about 215 KB as JSON).
