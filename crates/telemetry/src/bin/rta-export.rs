//! Turn recorded sessions into the JSON the site reads.
//!
//! ```text
//! rta-export [--sessions DIR] [--tracks FILE] --out DIR
//! ```
//! Writes `DIR/tracks.json` (every layout), `DIR/laps.json` (every lap, with traces) and
//! `DIR/data.js`, the same data as a classic script so the built site also works from `file://`,
//! where browsers block `fetch`. Defaults read from `~/.local/share/ride-time-attack/`.

use std::path::PathBuf;
use telemetry::{csv::read_session, laps, track::load_tracks};

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"))
        .join("ride-time-attack")
}

fn main() -> std::io::Result<()> {
    let mut sessions = data_dir().join("sessions");
    let mut tracks_path = data_dir().join("tracks.json");
    let mut out: Option<PathBuf> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sessions" => sessions = it.next().expect("--sessions DIR").into(),
            "--tracks" => tracks_path = it.next().expect("--tracks FILE").into(),
            "--out" => out = Some(it.next().expect("--out DIR").into()),
            _ => {
                eprintln!("rta-export [--sessions DIR] [--tracks FILE] --out DIR");
                std::process::exit(2);
            }
        }
    }
    let out = out.unwrap_or_else(|| { eprintln!("--out is required"); std::process::exit(2) });
    let tracks = load_tracks(&tracks_path)
        .map_err(|e| std::io::Error::other(format!("{}: {e}", tracks_path.display())))?;

    let mut files: Vec<PathBuf> = std::fs::read_dir(&sessions)?.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "csv")).collect();
    files.sort();
    let mut all = vec![];
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let laps = laps::cut(&name, read_session(f)?, &tracks);
        println!("{name}: {} laps{}", laps.len(), laps.first().map(|l| format!(" on {}", l.track)).unwrap_or_default());
        all.extend(laps);
    }

    std::fs::create_dir_all(&out)?;
    let used = tracks.keys().filter(|k| all.iter().any(|l| &l.track == *k)).count();
    serde_json::to_writer(std::fs::File::create(out.join("laps.json"))?, &all).map_err(std::io::Error::other)?;
    serde_json::to_writer(std::fs::File::create(out.join("tracks.json"))?, &tracks).map_err(std::io::Error::other)?;
    let js = format!(
        "window.RTA_DATA = {{\"tracks\":{},\"laps\":{}}};\n",
        serde_json::to_string(&tracks).map_err(std::io::Error::other)?,
        serde_json::to_string(&all).map_err(std::io::Error::other)?,
    );
    std::fs::write(out.join("data.js"), js)?;
    println!("{} laps ({} valid) on {used} layouts -> {}", all.len(), all.iter().filter(|l| l.valid).count(), out.display());
    Ok(())
}
