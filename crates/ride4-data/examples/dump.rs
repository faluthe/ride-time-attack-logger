//! Extract everything and write it as JSON for inspection.
//! `cargo run --release -p ride4-data --example dump -- GAME_DIR SAVES_DIR OUT_DIR`

use ride4_data::{events, saves, tracks, Game};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let (game, saves_dir, out) = (&a[0], &a[1], &a[2]);
    std::fs::create_dir_all(out)?;
    let g = Game::open(game)?;
    let t = tracks::extract(&g)?;
    let e = events::extract(&g)?;
    let lb = saves::leaderboards(saves_dir)?;
    let c = saves::career(saves_dir, &e)?;
    println!("{} tracks, {} events, {} leaderboards, {} career results", t.len(), e.len(), lb.len(), c.len());
    let w = |name: &str, v: serde_json::Value| std::fs::write(out.join(name), serde_json::to_string_pretty(&v).unwrap());
    w("tracks.json", serde_json::to_value(&t)?)?;
    w("events.json", serde_json::to_value(&e)?)?;
    w("leaderboards.json", serde_json::to_value(&lb)?)?;
    w("career.json", serde_json::to_value(&c)?)?;
    Ok(())
}
