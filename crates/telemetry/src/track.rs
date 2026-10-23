//! Track layouts (centreline and road edges, metres) and projection onto the centreline.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type P2 = [f64; 2];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub venue: String,
    pub layout: String,
    pub length_m: f64,
    /// Closed loop: the last point connects back to the first.
    pub centre: Vec<P2>,
    pub left: Vec<P2>,
    pub right: Vec<P2>,
}

/// All layouts by key (`kanto_c`), as in `tracks.json`.
pub type Tracks = BTreeMap<String, Track>;

pub fn load_tracks(path: &std::path::Path) -> std::io::Result<Tracks> {
    let f = std::fs::File::open(path)?;
    serde_json::from_reader(std::io::BufReader::new(f)).map_err(std::io::Error::other)
}

fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// A closed polyline with cumulative arc length, for projecting positions to lap distance.
pub struct Line {
    pub pts: Vec<P2>,
    cum: Vec<f64>,
    pub length: f64,
}

/// Result of projecting a point onto a `Line`.
#[derive(Clone, Copy, Debug)]
pub struct Projection {
    /// Arc length along the line, 0..length.
    pub s: f64,
    /// Distance from the line, metres.
    pub off: f64,
    /// Segment index, usable as a hint for the next projection.
    pub seg: usize,
}

impl Line {
    pub fn new(pts: &[P2]) -> Self {
        let n = pts.len();
        let mut cum = Vec::with_capacity(n + 1);
        cum.push(0.0);
        for i in 0..n {
            cum.push(cum[i] + dist(pts[i], pts[(i + 1) % n]));
        }
        Self { pts: pts.to_vec(), length: cum[n], cum }
    }

    fn project_seg(&self, i: usize, p: P2) -> (f64, f64) {
        let n = self.pts.len();
        let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = (dx * dx + dy * dy).max(1e-9);
        let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
        let d2 = (a[0] + u * dx - p[0]).powi(2) + (a[1] + u * dy - p[1]).powi(2);
        (d2, u)
    }

    /// Nearest point on the line. With a `hint`, only segments within `window` of it are searched.
    pub fn project(&self, p: P2, hint: Option<usize>, window: usize) -> Projection {
        let n = self.pts.len();
        let mut best = (f64::MAX, 0, 0.0);
        let mut check = |i: usize| {
            let (d2, u) = self.project_seg(i, p);
            if d2 < best.0 {
                best = (d2, i, u);
            }
        };
        match hint {
            None => (0..n).for_each(&mut check),
            Some(h) => (0..2 * window).for_each(|k| check((h + n + k - window) % n)),
        }
        let (d2, i, u) = best;
        Projection { s: self.cum[i] + u * (self.cum[i + 1] - self.cum[i]), off: d2.sqrt(), seg: i }
    }
}
