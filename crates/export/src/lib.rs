//! Turn the game's files, your saves and recorded sessions into the data the site reads.
//!
//! Track layouts and career event tables come from the game paks. They're extracted once into
//! `~/.local/share/ride-time-attack/` and reused; [`Options::extract`] redoes it (after a game
//! update). Saves (offline leaderboards, career results) and sessions (logged laps) are read
//! every run.
//!
//! Writes `tracks.json`, `laps.json`, `leaderboards.json`, `career.json` and `laps/<id>.bin.gz`.
//! `rta-export` runs it from the command line; the logger runs it after each lap to upload.

use ride4_data::{events, saves, tracks, Game};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use telemetry::{csv::read_session, lapfile, laps};

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// `~/.local/share/ride-time-attack` (or under `$XDG_DATA_HOME`).
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("ride-time-attack")
}

fn steam() -> PathBuf {
    home().join(".local/share/Steam/steamapps")
}

pub struct Options {
    pub out: PathBuf,
    pub game: PathBuf,
    pub saves: PathBuf,
    pub sessions: PathBuf,
    /// re-extract tracks and events from the paks instead of using the cached copies
    pub extract: bool,
}

impl Options {
    /// The usual Steam install and save locations, and the logger's sessions.
    pub fn new(out: PathBuf) -> Options {
        Options {
            out,
            game: steam().join("common/RIDE 4"),
            saves: steam().join("compatdata/1259980/pfx/drive_c/users/steamuser/AppData/Local/Ride4/Saved/SaveGames"),
            sessions: data_dir().join("sessions"),
            extract: false,
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Res<T> {
    Ok(serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(p)?))?)
}

fn write_json(p: &Path, v: &impl Serialize) -> Res<()> {
    serde_json::to_writer(std::io::BufWriter::new(std::fs::File::create(p)?), v)?;
    Ok(())
}

/// Layouts and career events: cached extraction from the game paks.
fn game_tables(o: &Options) -> Res<(telemetry::track::Tracks, events::Events)> {
    let (tp, ep) = (data_dir().join("tracks.json"), data_dir().join("events.json"));
    if !o.extract && tp.exists() && ep.exists() {
        return Ok((read_json(&tp)?, read_json(&ep)?));
    }
    println!("extracting tracks and career events from {}", o.game.display());
    let game = Game::open(&o.game)?;
    let (t, e) = (tracks::extract(&game)?, events::extract(&game)?);
    std::fs::create_dir_all(data_dir())?;
    write_json(&tp, &t)?;
    write_json(&ep, &e)?;
    println!("  {} layouts, {} events -> {}", t.len(), e.len(), data_dir().display());
    Ok((t, e))
}

/// `laps.json`: every lap without its trace, plus where to find the trace.
#[derive(Serialize)]
struct LapIndex {
    /// Unix seconds the newest session with laps was last written; absent without laps
    #[serde(skip_serializing_if = "Option::is_none")]
    generated: Option<u64>,
    laps: Vec<serde_json::Value>,
}

/// A lap file: the encoded trace, gzipped (the browser undoes it with `DecompressionStream`).
fn lap_file(tr: &laps::Trace) -> Res<Vec<u8>> {
    use std::io::Write;
    let raw = lapfile::encode(tr);
    if lapfile::decode(&raw)? != *tr {
        return Err("trace doesn't survive encoding".into());
    }
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&raw)?;
    Ok(z.finish()?)
}

/// Write each lap's trace to `laps/<id>.bin.gz` (removing stale ones) and return the index entries.
fn write_laps(out: &Path, laps: &[laps::Lap]) -> Res<Vec<serde_json::Value>> {
    let dir = out.join("laps");
    std::fs::create_dir_all(&dir)?;
    let mut index = vec![];
    let mut written = std::collections::HashSet::new();
    let mut bytes = 0;
    for lap in laps {
        let name = format!("{}.bin.gz", lap.id);
        let b = lap_file(&lap.trace).map_err(|e| format!("lap {}: {e}", lap.id))?;
        std::fs::write(dir.join(&name), &b)?;
        bytes += b.len();
        let mut v = serde_json::to_value(lap)?;
        let o = v.as_object_mut().unwrap();
        o.remove("trace");
        o.insert("samples".into(), lap.trace.t.len().into());
        o.insert("file".into(), format!("laps/{name}").into());
        index.push(v);
        written.insert(name);
    }
    for e in std::fs::read_dir(&dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if (name.ends_with(".bin.gz") || name.ends_with(".bin")) && !written.contains(&name) {
            std::fs::remove_file(e.path())?;
        }
    }
    println!("{} lap files, {} KB -> {}", laps.len(), bytes / 1024, dir.display());
    Ok(index)
}

/// Unix seconds a file was last written.
fn modified(p: &Path) -> Option<u64> {
    Some(std::fs::metadata(p).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// Write everything to `o.out`.
pub fn export(o: &Options) -> Res<()> {
    let (tracks, events) = game_tables(o)?;

    let (leaderboards, career) = match (saves::leaderboards(&o.saves), saves::career(&o.saves, &events)) {
        (Ok(l), Ok(c)) => (l, c),
        (l, c) => {
            for e in [l.err(), c.err()].into_iter().flatten() {
                eprintln!("skipping saves: {e}");
            }
            (vec![], vec![])
        }
    };
    println!("saves: {} leaderboards, {} career results", leaderboards.len(), career.len());

    let mut files: Vec<PathBuf> = match std::fs::read_dir(&o.sessions) {
        Ok(d) => d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "csv")).collect(),
        Err(e) => {
            eprintln!("no sessions in {}: {e}", o.sessions.display());
            vec![]
        }
    };
    files.sort();
    let mut logged = vec![];
    // when the newest lap was recorded, so an export without new laps gives an identical index
    let mut generated = None;
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let laps = laps::cut(&name, read_session(f)?, &tracks);
        println!("session {name}: {} laps{}", laps.len(), laps.first().map(|l| format!(" on {}", l.track)).unwrap_or_default());
        if !laps.is_empty() {
            generated = generated.max(modified(f));
        }
        logged.extend(laps);
    }

    std::fs::create_dir_all(&o.out)?;
    write_json(&o.out.join("tracks.json"), &tracks)?;
    write_json(&o.out.join("leaderboards.json"), &leaderboards)?;
    write_json(&o.out.join("career.json"), &career)?;
    let laps = write_laps(&o.out, &logged)?;
    write_json(&o.out.join("laps.json"), &LapIndex { generated, laps })?;
    // the old all-in-one bundle for file://; lap traces are separate files now
    if std::fs::remove_file(o.out.join("data.js")).is_ok() {
        println!("removed stale data.js");
    }
    println!("{} logged laps ({} valid) -> {}", logged.len(), logged.iter().filter(|l| l.valid).count(), o.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn lap_file_is_gzipped_lapfile() {
        let tr = laps::Trace {
            t: vec![0.0, 0.04, 0.081],
            d: vec![-1.2, 0.3, 1.9],
            x: vec![100.25, 101.5, 102.75],
            y: vec![-5.0, -5.01, -5.02],
            v: vec![140.2, 140.3, 140.5],
            lean: vec![-3.1, -3.0, -2.8],
            ax: vec![0.12, 0.1, 0.09],
            thr: Some(vec![1.0, 1.0, 0.97]),
            brk: Some(vec![0.0, 0.0, 0.0]),
            rear: Some(vec![0, 0, 1]),
            gear: Some(vec![4, 4, 5]),
            rpm: Some(vec![0.912, 0.95, 0.701]),
        };
        let gz = lap_file(&tr).unwrap();
        assert_eq!(gz[..2], [0x1f, 0x8b]);
        let mut raw = vec![];
        flate2::read::GzDecoder::new(&gz[..]).read_to_end(&mut raw).unwrap();
        assert_eq!(lapfile::decode(&raw).unwrap(), tr);
    }
}
