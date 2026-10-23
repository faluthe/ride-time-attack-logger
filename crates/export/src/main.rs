//! Turn the game's files, your saves and recorded sessions into the data the site reads.
//!
//! ```text
//! rta-export --out DIR [--game DIR] [--saves DIR] [--sessions DIR] [--extract]
//! ```
//!
//! Track layouts and career event tables come from the game paks. They're extracted once into
//! `~/.local/share/ride-time-attack/` and reused; `--extract` redoes it (after a game update).
//! Saves (offline leaderboards, career results) and sessions (logged laps) are read every run.
//!
//! Writes `tracks.json`, `laps.json`, `leaderboards.json`, `career.json` and `data.js` (all of it
//! as one classic script, so the built site also works from `file://`, where `fetch` is blocked).

use ride4_data::{events, saves, tracks, Game};
use serde::Serialize;
use std::path::{Path, PathBuf};
use telemetry::{csv::read_session, laps};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "rta-export --out DIR [--game DIR] [--saves DIR] [--sessions DIR] [--extract]";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("ride-time-attack")
}

fn steam() -> PathBuf {
    home().join(".local/share/Steam/steamapps")
}

struct Args {
    out: PathBuf,
    game: PathBuf,
    saves: PathBuf,
    sessions: PathBuf,
    extract: bool,
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}

fn args() -> Args {
    let mut out = None;
    let mut a = Args {
        out: PathBuf::new(),
        game: steam().join("common/RIDE 4"),
        saves: steam().join("compatdata/1259980/pfx/drive_c/users/steamuser/AppData/Local/Ride4/Saved/SaveGames"),
        sessions: data_dir().join("sessions"),
        extract: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().map(PathBuf::from).unwrap_or_else(|| usage());
        match arg.as_str() {
            "--out" => out = Some(val()),
            "--game" => a.game = val(),
            "--saves" => a.saves = val(),
            "--sessions" => a.sessions = val(),
            "--extract" => a.extract = true,
            _ => usage(),
        }
    }
    a.out = out.unwrap_or_else(|| usage());
    a
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Res<T> {
    Ok(serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(p)?))?)
}

fn write_json(p: &Path, v: &impl Serialize) -> Res<()> {
    serde_json::to_writer(std::io::BufWriter::new(std::fs::File::create(p)?), v)?;
    Ok(())
}

/// Layouts and career events: cached extraction from the game paks.
fn game_tables(a: &Args) -> Res<(telemetry::track::Tracks, events::Events)> {
    let (tp, ep) = (data_dir().join("tracks.json"), data_dir().join("events.json"));
    if !a.extract && tp.exists() && ep.exists() {
        return Ok((read_json(&tp)?, read_json(&ep)?));
    }
    println!("extracting tracks and career events from {}", a.game.display());
    let game = Game::open(&a.game)?;
    let (t, e) = (tracks::extract(&game)?, events::extract(&game)?);
    std::fs::create_dir_all(data_dir())?;
    write_json(&tp, &t)?;
    write_json(&ep, &e)?;
    println!("  {} layouts, {} events -> {}", t.len(), e.len(), data_dir().display());
    Ok((t, e))
}

#[derive(Serialize)]
struct SiteData<'a> {
    /// Unix seconds
    generated: u64,
    tracks: &'a telemetry::track::Tracks,
    laps: &'a [laps::Lap],
    leaderboards: &'a [saves::Leaderboard],
    career: &'a [saves::CareerResult],
}

fn main() -> Res<()> {
    let a = args();
    let (tracks, events) = game_tables(&a)?;

    let (leaderboards, career) = match (saves::leaderboards(&a.saves), saves::career(&a.saves, &events)) {
        (Ok(l), Ok(c)) => (l, c),
        (l, c) => {
            for e in [l.err(), c.err()].into_iter().flatten() {
                eprintln!("skipping saves: {e}");
            }
            (vec![], vec![])
        }
    };
    println!("saves: {} leaderboards, {} career results", leaderboards.len(), career.len());

    let mut files: Vec<PathBuf> = match std::fs::read_dir(&a.sessions) {
        Ok(d) => d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "csv")).collect(),
        Err(e) => {
            eprintln!("no sessions in {}: {e}", a.sessions.display());
            vec![]
        }
    };
    files.sort();
    let mut logged = vec![];
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let laps = laps::cut(&name, read_session(f)?, &tracks);
        println!("session {name}: {} laps{}", laps.len(), laps.first().map(|l| format!(" on {}", l.track)).unwrap_or_default());
        logged.extend(laps);
    }

    std::fs::create_dir_all(&a.out)?;
    write_json(&a.out.join("tracks.json"), &tracks)?;
    write_json(&a.out.join("laps.json"), &logged)?;
    write_json(&a.out.join("leaderboards.json"), &leaderboards)?;
    write_json(&a.out.join("career.json"), &career)?;
    let generated = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let all = SiteData { generated, tracks: &tracks, laps: &logged, leaderboards: &leaderboards, career: &career };
    std::fs::write(a.out.join("data.js"), format!("window.RTA_DATA = {};\n", serde_json::to_string(&all)?))?;
    println!("{} logged laps ({} valid) -> {}", logged.len(), logged.iter().filter(|l| l.valid).count(), a.out.display());
    Ok(())
}
