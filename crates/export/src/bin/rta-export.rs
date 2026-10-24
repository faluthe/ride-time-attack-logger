//! ```text
//! rta-export --out DIR [--game DIR] [--saves DIR] [--sessions DIR] [--extract] [--upload]
//! ```
//!
//! Exports to `DIR` (see the `export` library). `--upload` then sends what changed to the site's
//! R2 bucket (see the `upload` crate for the `.env` it needs).

use export::{Options, Res};
use std::path::PathBuf;

const USAGE: &str = "rta-export --out DIR [--game DIR] [--saves DIR] [--sessions DIR] [--extract] [--upload]";

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}

fn args() -> (Options, bool) {
    let mut out = None;
    let mut o = Options::new(PathBuf::new());
    let mut upload = false;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().map(PathBuf::from).unwrap_or_else(|| usage());
        match arg.as_str() {
            "--out" => out = Some(val()),
            "--game" => o.game = val(),
            "--saves" => o.saves = val(),
            "--sessions" => o.sessions = val(),
            "--extract" => o.extract = true,
            "--upload" => upload = true,
            _ => usage(),
        }
    }
    o.out = out.unwrap_or_else(|| usage());
    (o, upload)
}

fn main() -> Res<()> {
    let (o, upload) = args();
    // check the credentials before the export, not after
    let store = if upload { Some(upload::R2::from_env()?) } else { None };
    export::export(&o)?;
    if let Some(store) = store {
        println!("uploading to R2 bucket {}", store.bucket());
        let s = upload::sync(&store, &o.out)?;
        println!("  {} uploaded ({} KB), {} unchanged, {} deleted", s.uploaded.len(), s.bytes / 1024, s.unchanged, s.deleted.len());
    }
    Ok(())
}
