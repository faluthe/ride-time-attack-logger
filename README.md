# Ride Time Attack: logger

Live telemetry for **Ride 4** on Linux (Steam/Proton). It reads the player's bike straight out of
the game's memory at 50 Hz, shows it on a Dear ImGui dashboard, records sessions to CSV, and cuts
them into laps for the site.

```sh
cargo run --release --bin rta-logger                # dashboard; records while you're on track
cargo run --release --bin rta-export -- --out ../site/public/data
cargo run --release -p ride4-mem --example probe     # text readout, handy after a game update
```

`rta-logger` options: `--sessions DIR`, `--no-record`, `--light`, and
`--screenshot FILE.png [--shot-delay S]`, which saves one frame and exits.

Data lives in `~/.local/share/ride-time-attack/`:
- `sessions/*.csv`: one file per stint on track
- `tracks.json`: layout centrelines and road edges. Copied from the old prototype for now, until
  the pak extractor is ported.

## Crates

| Crate | What it does |
|---|---|
| `ride4-mem` | `/proc/<pid>/mem` access, FNamePool lookups, and the GWorld → pawn pointer chain (`src/offsets.rs`) |
| `telemetry` | samples, CSV sessions, track projection, lap cutting, the `rta-export` binary |
| `app` | `rta-logger`: reader thread plus the ImGui dashboard (winit + glutin + glow) |

## How the bike is found

The exe loads at `0x140000000` (no ASLR), so the root is a fixed global:

```
GWorld 0x145c80d80 → +0x188 GameInstance → +0x38 LocalPlayers[0] → +0x30 PlayerController
       → +0x250 Pawn (BaseMotorBike_C; time-trial ghosts are separate actors)
```

| Channel | Path from the bike | Type |
|---|---|---|
| Position, rotation | `+0x130` mesh → quat `+0x1C0`, translation `+0x1D0` | f32, cm |
| Throttle, brake, rear | `+0x290` movement → `+0x1DC`, `+0x1E0`, `+0x1E4` | f32 0..1 (rear is 0/1) |
| Gear | `+0x8F0` IgnitionBike → `+0x264` | i32, 0 = neutral |
| Engine speed | `+0x1070` audio component → `+0x238` | f32, idle 0 .. limiter 1 |

If an update moves GWorld, the reader rescans `.data` for a pointer whose chain ends at a
`BaseMotorBike_C` (checked by class name).

## CSV columns

`t,x,y,z,fx,fy,fz,side_z,up_z,throttle,brake,rear,gear,rpm`: time (s), position (m), forward
vector, z of the side and up axes (lean = −asin(side_z)), then the inputs. Older 9-column files
still load.

## Not ported yet

- Track extraction from the game paks (`IgnitionSplineMesh`) → `tracks.json`
- Save parsing (GVAS): offline leaderboards and career results
- The game's own lap timer (laps are currently cut at the spline start, about ±0.1 s)
