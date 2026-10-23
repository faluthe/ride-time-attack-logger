# Ride Time Attack: logger

Live telemetry and data tools for **Ride 4** on Linux (Steam/Proton). The logger reads the
player's bike and the game's lap timing straight out of memory at 50 Hz, shows them on a Dear
ImGui dashboard and records sessions to CSV. `rta-export` turns sessions, your save games and
the game's own files into the data the site shows.

```sh
cargo run --release --bin rta-logger                 # dashboard; records while you're on track
cargo run --release --bin rta-export -- --out ../site/public/data
cargo run --release -p ride4-mem --example probe      # text readout, handy after a game update
```

`rta-logger` options: `--sessions DIR`, `--no-record`, `--light`, and
`--screenshot FILE.png [--shot-delay S]`, which saves one frame and exits.

`rta-export` options: `--game DIR`, `--saves DIR`, `--sessions DIR`, and `--extract`, which
re-reads tracks and events from the game paks after a game update.

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
