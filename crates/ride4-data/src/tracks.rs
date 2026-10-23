//! Track layouts from the minimap spline meshes in the game paks.

use crate::pak::Game;
use crate::reader::{err, Reader, Result};
use std::collections::BTreeMap;
use telemetry::track::{Track, Tracks};

const MAPS: &str = "ride4/Content/assets/ui/Images/Libraries/maps/";

/// Venue display names by key prefix (`kanto_c` -> "Kanto").
const VENUES: &[(&str, &str)] = &[
    ("brandshatch", "Brands Hatch"), ("cadwellpark", "Cadwell Park"), ("canadian", "Canadian Tire Motorsport Park"),
    ("daytona", "Daytona"), ("donington", "Donington Park"), ("frenchriviera", "French Riviera"), ("imatra", "Imatra"),
    ("imola", "Imola"), ("interlagos", "Interlagos"), ("kanto", "Kanto"), ("kyalami", "Kyalami"),
    ("magnycours", "Magny-Cours"), ("monza", "Monza"), ("mugello", "Mugello"), ("nordschleife", "Nürburgring Nordschleife"),
    ("northwest200", "North West 200"), ("nurburgringgp", "Nürburgring GP"), ("okayama", "Okayama"),
    ("oultonpark", "Oulton Park"), ("phillipisland", "Phillip Island"), ("portimao", "Portimão"),
    ("roadamerica", "Road America"), ("roadatlanta", "Road Atlanta"), ("snetterton", "Snetterton"),
    ("southern100", "Southern 100"), ("sugo", "Sportsland SUGO"), ("suzuka", "Suzuka"), ("tsukuba", "Tsukuba"),
    ("ulstergp", "Ulster GP"), ("utah", "Utah Motorsports Campus"), ("valencia", "Valencia"),
    ("virginia", "Virginia International Raceway"), ("lagunaseca", "Laguna Seca"), ("macau", "Macau"),
];

/// Layout key as used everywhere (`Kanto_C` -> `kanto_c`).
pub fn track_key(s: &str) -> String {
    s.to_lowercase()
}

/// `kanto_c` -> ("Kanto", "C")
pub fn track_label(key: &str) -> (String, String) {
    let (venue, layout) = key.rsplit_once('_').unwrap_or((key, ""));
    let name = VENUES.iter().find(|(k, _)| *k == venue).map(|(_, v)| v.to_string()).unwrap_or_else(|| {
        let mut c = venue.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    });
    (name, layout.to_uppercase())
}

/// `IgnitionSplineMesh` export: three bulk `Vector2D` arrays (centreline in cm, unit normals, and
/// the left/right edge offsets along the normal).
fn spline(b: &[u8]) -> Result<[Vec<[f32; 2]>; 3]> {
    let mut r = Reader::new(b);
    r.p = 0x56;
    let mut read = || -> Result<Vec<[f32; 2]>> {
        let (elem, n) = (r.i32()?, r.i32()?);
        if elem != 8 {
            return err(format!("unexpected element size {elem}"));
        }
        (0..n).map(|_| Ok([r.f32()?, r.f32()?])).collect()
    };
    Ok([read()?, read()?, read()?])
}

/// Round to centimetres. Formatting rounds the exact binary value correctly, which
/// `(v * 100).round() / 100` does not on ties.
fn round2(v: f64) -> f64 {
    format!("{v:.2}").parse().unwrap()
}

pub fn extract(game: &Game) -> Result<Tracks> {
    let mut out = BTreeMap::new();
    for name in game.names() {
        let Some(rest) = name.strip_prefix(MAPS) else { continue };
        if !rest.contains("/SplineMeshes/") || !rest.ends_with(".uexp") {
            continue;
        }
        let folder = rest.split('/').next().unwrap();
        let base = rest.rsplit('/').next().unwrap().trim_end_matches(".uexp");
        if !base.eq_ignore_ascii_case(folder) {
            continue; // e.g. a legacy "Mugello" mesh next to "Mugello_A"
        }
        let [centre, normal, width] = spline(&game.read(name)?)?;
        let m = |v: f64| round2(v / 100.0); // cm -> m
        let edge = |side: usize| -> Vec<[f64; 2]> {
            centre.iter().zip(&normal).zip(&width)
                .map(|((c, n), w)| {
                    let (cx, cy, w) = (c[0] as f64, c[1] as f64, w[side] as f64);
                    [m(cx + n[0] as f64 * w), m(cy + n[1] as f64 * w)]
                }).collect()
        };
        let c: Vec<[f64; 2]> = centre.iter().map(|p| [m(p[0] as f64), m(p[1] as f64)]).collect();
        let length: f64 = c.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        let key = track_key(folder);
        let (venue, layout) = track_label(&key);
        out.insert(key, Track { venue, layout, length_m: length.round(), centre: c, left: edge(0), right: edge(1) });
    }
    Ok(out)
}
