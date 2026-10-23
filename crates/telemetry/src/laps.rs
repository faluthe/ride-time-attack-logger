//! Cut a recorded session into laps on the layout that was ridden.

use crate::sample::Sample;
use crate::track::{Line, Tracks, P2};
use serde::Serialize;
use std::collections::HashSet;

/// Position jump between frames that means a reset / rewind, metres.
const RESET_JUMP_M: f64 = 30.0;
/// Progress going backwards by more than this invalidates the lap, metres.
const BACKWARDS_M: f64 = 40.0;
/// Samples per second kept in exported traces.
const OUT_HZ: f64 = 25.0;
/// No new frames for this long = paused (the game's lap timer stops; ours wouldn't).
const PAUSE_S: f64 = 1.0;
/// Worst acceptable 90th-percentile distance from a layout's centreline, metres.
const MAX_OFF_LINE_M: f64 = 25.0;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Trace {
    pub t: Vec<f64>,
    /// Lap distance from the start line, metres (slightly negative just before it).
    pub d: Vec<f64>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// km/h
    pub v: Vec<f64>,
    /// degrees, right positive
    pub lean: Vec<f64>,
    /// longitudinal g
    pub ax: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thr: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brk: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rear: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gear: Option<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm: Option<Vec<f64>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Lap {
    pub id: String,
    pub session: String,
    pub lap: usize,
    pub track: String,
    pub valid: bool,
    pub problem: Option<String>,
    /// seconds
    pub time: f64,
    pub top_speed: f64,
    pub avg_speed: f64,
    pub max_lean_left: f64,
    pub max_lean_right: f64,
    pub max_brake_g: f64,
    pub max_accel_g: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_throttle_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub braking_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_gear: Option<i32>,
    pub trace: Trace,
}

fn xy(s: &Sample) -> P2 {
    [s.pos[0] as f64, s.pos[1] as f64]
}

fn dist2(a: &Sample, b: &Sample) -> f64 {
    let (p, q) = (xy(a), xy(b));
    (p[0] - q[0]).hypot(p[1] - q[1])
}

fn dist3(a: &Sample, b: &Sample) -> f64 {
    (0..3).map(|i| (a.pos[i] as f64 - b.pos[i] as f64).powi(2)).sum::<f64>().sqrt()
}

fn round(v: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (v * m).round() / m
}

/// Drop frames where the old shared physics slot briefly held another bike. Harmless on sessions
/// from the object-graph logger, which never reads another bike, so it's applied to everything.
///
/// A frame is kept if it's reachable from the last kept frame at <= `max_speed` m/s. Frames that
/// aren't are held back; if `confirm` of them in a row agree with each other, it was a genuine
/// relocation (reset / rewind) and they're kept, otherwise they're dropped as glitches.
pub fn despike(rows: Vec<Sample>, max_speed: f64, confirm: usize) -> Vec<Sample> {
    let mut it = rows.into_iter();
    let Some(first) = it.next() else { return vec![] };
    let mut out = vec![first];
    let mut pending: Vec<Sample> = vec![];
    let reach = |a: &Sample, b: &Sample| dist3(a, b) <= max_speed * (b.t - a.t).max(0.02) + 5.0;
    for r in it {
        if reach(out.last().unwrap(), &r) {
            out.push(r);
            pending.clear();
            continue;
        }
        if pending.last().is_some_and(|p| !reach(p, &r)) {
            pending.clear();
        }
        pending.push(r);
        if pending.len() >= confirm {
            out.append(&mut pending);
        }
    }
    out
}

/// The layout this session was ridden on: `(90th percentile distance, key, line)`.
///
/// Layouts can share road (all of Kanto C lies on Kanto A), so candidates are ranked by how much
/// of their centreline was covered first, and by distance second.
pub fn pick_track(rows: &[Sample], tracks: &Tracks) -> Option<(f64, String, Line)> {
    let step = (rows.len() / 300).max(1);
    let sample: Vec<P2> = rows.iter().step_by(step).map(xy).collect();
    let (xmin, xmax) = sample.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let (ymin, ymax) = sample.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let cell = |x: f64, y: f64| ((x / 20.0).round() as i64, (y / 20.0).round() as i64);
    let visited: HashSet<_> = rows.iter().map(|r| cell(r.pos[0] as f64, r.pos[1] as f64)).collect();

    let mut best: Option<((i64, f64), f64, String, Line)> = None;
    for (key, t) in tracks {
        let c = &t.centre;
        let (cxmin, cxmax) = c.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
        let (cymin, cymax) = c.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        if xmax < cxmin - 50.0 || xmin > cxmax + 50.0 || ymax < cymin - 50.0 || ymin > cymax + 50.0 {
            continue;
        }
        let line = Line::new(c);
        let mut ds: Vec<f64> = sample.iter().map(|&p| line.project(p, None, 0).off).collect();
        ds.sort_by(f64::total_cmp);
        let score = ds[(ds.len() as f64 * 0.9) as usize];
        if score > MAX_OFF_LINE_M {
            continue;
        }
        let pts: Vec<&P2> = c.iter().step_by((c.len() / 400).max(1)).collect();
        let near = pts.iter().filter(|p| {
            let (i, j) = cell(p[0], p[1]);
            (-1..=1).any(|di| (-1..=1).any(|dj| visited.contains(&(i + di, j + dj))))
        }).count();
        // coverage rounded to whole percent so near-equal layouts fall through to distance
        let rank = (-((100 * near / pts.len()) as i64), score);
        if best.as_ref().map_or(true, |b| rank.0 < b.0 .0 || (rank.0 == b.0 .0 && rank.1 < b.0 .1)) {
            best = Some((rank, score, key.clone(), line));
        }
    }
    best.map(|(_, s, k, l)| (s, k, l))
}

/// Cut one session into laps.
pub fn cut(session: &str, rows: Vec<Sample>, tracks: &Tracks) -> Vec<Lap> {
    let rows = despike(rows, 100.0, 25);
    if rows.len() < 100 {
        return vec![];
    }
    let Some((_, key, line)) = pick_track(&rows, tracks) else { return vec![] };
    let l = line.length;

    // lap distance for every frame; drop the search hint after a teleport
    let mut prog = Vec::with_capacity(rows.len());
    let mut hint = None;
    for (i, r) in rows.iter().enumerate() {
        if i > 0 && dist2(&rows[i - 1], r) > RESET_JUMP_M {
            hint = None;
        }
        let p = line.project(xy(r), hint, 25);
        hint = Some(p.seg);
        prog.push(p.s);
    }
    let wrap = |d: f64| (d + l / 2.0).rem_euclid(l) - l / 2.0;
    // ride direction relative to the spline
    let fwd: i64 = prog.windows(2).filter(|w| w[0] != w[1]).map(|w| if wrap(w[1] - w[0]) > 0.0 { 1 } else { -1 }).sum();
    if fwd < 0 {
        for s in &mut prog {
            *s = (l - *s).rem_euclid(l);
        }
    }

    // start/finish crossings with interpolated time
    let mut crossings = vec![];
    for i in 1..rows.len() {
        let (a, b) = (prog[i - 1], prog[i]);
        if a > 0.8 * l && b < 0.2 * l && dist2(&rows[i - 1], &rows[i]) < RESET_JUMP_M {
            let f = (l - a) / ((l - a) + b);
            crossings.push((i, rows[i - 1].t + f * (rows[i].t - rows[i - 1].t)));
        }
    }

    crossings.windows(2).enumerate().map(|(n, w)| {
        let ((i0, t0), (i1, t1)) = (w[0], w[1]);
        let problem = (i0 + 1..i1).find_map(|j| {
            if wrap(prog[j] - prog[j - 1]) < -BACKWARDS_M || dist2(&rows[j], &rows[j - 1]) > RESET_JUMP_M {
                Some("reset or rewind")
            } else if rows[j].t - rows[j - 1].t > PAUSE_S {
                Some("paused or stopped")
            } else {
                None
            }
        });
        export(&rows, &prog, i0..i1, t0, t1, &key, session, n + 1, problem, l)
    }).collect()
}

/// Least-squares fit of s = a0 + a1 u + a2 u² around one sample -> (speed km/h, accel g).
///
/// Logger ticks aren't aligned with game frames, so plain differences alias into noise; distance
/// itself is clean, and a ±0.5 s fit smooths it without lag.
fn kinematics(times: &[f64], cum: &[f64], c: usize, half: f64) -> (f64, f64) {
    let (mut lo, mut hi) = (c, c);
    while lo > 0 && times[c] - times[lo] < half {
        lo -= 1;
    }
    while hi < times.len() - 1 && times[hi] - times[c] < half {
        hi += 1;
    }
    let mut s = [0.0; 5];
    let mut b = [0.0; 3];
    for k in lo..=hi {
        let u = times[k] - times[c];
        let mut p = 1.0;
        for e in 0..5 {
            s[e] += p;
            if e < 3 {
                b[e] += p * cum[k];
            }
            p *= u;
        }
    }
    let m = [[s[0], s[1], s[2]], [s[1], s[2], s[3]], [s[2], s[3], s[4]]];
    match solve3(m, b) {
        Some([_, a1, a2]) => (a1 * 3.6, 2.0 * a2 / 9.81),
        None => (0.0, 0.0),
    }
}

fn det3(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Cramer's rule.
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let d = det3(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let col = |c: usize| {
        let mut mm = m;
        for r in 0..3 {
            mm[r][c] = b[r];
        }
        det3(mm) / d
    };
    Some([col(0), col(1), col(2)])
}

#[allow(clippy::too_many_arguments)]
fn export(rows: &[Sample], prog: &[f64], seg: std::ops::Range<usize>, t0: f64, t1: f64, key: &str, session: &str,
    n: usize, problem: Option<&str>, l: f64) -> Lap {
    let idx: Vec<usize> = seg.collect();
    let mut cum = vec![0.0];
    for w in idx.windows(2) {
        cum.push(cum.last().unwrap() + dist3(&rows[w[0]], &rows[w[1]]));
    }
    let times: Vec<f64> = idx.iter().map(|&k| rows[k].t).collect();
    let step = if t1 > t0 { ((idx.len() as f64 / ((t1 - t0) * OUT_HZ)).round() as usize).max(1) } else { 1 };
    let mid = idx[idx.len() / 2];
    let inputs = rows[idx[0]].inputs.is_some();

    let mut tr = Trace::default();
    if inputs {
        tr.thr = Some(vec![]);
        tr.brk = Some(vec![]);
        tr.rear = Some(vec![]);
        tr.gear = Some(vec![]);
        tr.rpm = Some(vec![]);
    }
    for c in (0..idx.len()).step_by(step) {
        let j = idx[c];
        let r = &rows[j];
        tr.t.push(round(r.t - t0, 3));
        tr.d.push(round(if prog[j] < 0.9 * l || j > mid { prog[j] } else { prog[j] - l }, 1));
        tr.x.push(round(r.pos[0] as f64, 2));
        tr.y.push(round(r.pos[1] as f64, 2));
        let (v, ax) = kinematics(&times, &cum, c, 0.5);
        tr.v.push(round(v, 1));
        tr.ax.push(round(ax, 2));
        tr.lean.push(round(r.lean_deg() as f64, 1));
        if let Some(i) = r.inputs {
            tr.thr.as_mut().unwrap().push(round(i.throttle as f64, 2));
            tr.brk.as_mut().unwrap().push(round(i.brake as f64, 2));
            tr.rear.as_mut().unwrap().push((i.rear > 0.5) as u8);
            tr.gear.as_mut().unwrap().push(i.gear);
            tr.rpm.as_mut().unwrap().push(round(i.rpm as f64, 3));
        }
    }
    let max = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);
    let min = |v: &[f64]| v.iter().cloned().fold(f64::MAX, f64::min);
    let pct = |f: &dyn Fn(&Sample) -> bool| round(100.0 * idx.iter().filter(|&&k| f(&rows[k])).count() as f64 / idx.len() as f64, 1);
    let (full_throttle_pct, braking_pct, top_gear) = if inputs {
        (
            Some(pct(&|s| s.inputs.unwrap().throttle > 0.95)),
            Some(pct(&|s| { let i = s.inputs.unwrap(); i.brake > 0.05 || i.rear > 0.5 })),
            idx.iter().map(|&k| rows[k].inputs.unwrap().gear).max(),
        )
    } else {
        (None, None, None)
    };
    Lap {
        id: format!("{session}-{n}"),
        session: session.into(),
        lap: n,
        track: key.into(),
        valid: problem.is_none(),
        problem: problem.map(Into::into),
        time: round(t1 - t0, 3),
        top_speed: max(&tr.v),
        avg_speed: round(l / (t1 - t0) * 3.6, 1),
        max_lean_left: min(&tr.lean),
        max_lean_right: max(&tr.lean),
        max_brake_g: -min(&tr.ax),
        max_accel_g: max(&tr.ax),
        full_throttle_pct,
        braking_pct,
        top_gear,
        trace: tr,
    }
}
