//! How often the game moves the bike, and whether the race clock steps in the same frame.
//!
//! Polls as fast as it can for a few seconds while you ride, then reports frames per second (the
//! position changing), the frame interval spread, and how often a position change and a race
//! clock change landed in different polls (a read straddling a frame).
//!
//! `cargo run --release -p ride4-mem --example framerate [seconds]`
use ride4_mem::{LinuxProcess, Session, EXE};
use std::time::{Duration, Instant};

fn main() {
    let secs: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let pid = LinuxProcess::find(EXE).expect("Ride 4 is not running");
    let mut s = Session::attach(pid).expect("attach");
    s.poll().expect("no bike: get on track first");

    let (t0, end) = (Instant::now(), Duration::from_secs_f64(secs));
    let (mut polls, mut last_pos, mut last_clock) = (0u64, None, None);
    let mut ticks = vec![];
    let (mut frames, mut clock_steps, mut together, mut pos_only, mut clock_only) = (vec![], 0, 0, 0, 0);
    while t0.elapsed() < end {
        let Some(b) = s.poll() else { continue };
        let clock = s.timing().map(|t| t.race_clock);
        polls += 1;
        let moved = last_pos.is_some_and(|p| p != b.pos);
        let ticked = clock.is_some() && last_clock.is_some_and(|c| Some(c) != clock);
        if moved {
            frames.push(t0.elapsed().as_secs_f64());
        }
        clock_steps += ticked as u32;
        if ticked {
            ticks.push(t0.elapsed().as_secs_f64());
        }
        match (moved, ticked) {
            (true, true) => together += 1,
            (true, false) => pos_only += 1,
            (false, true) => clock_only += 1,
            _ => {}
        }
        last_pos = Some(b.pos);
        last_clock = clock;
    }
    let el = t0.elapsed().as_secs_f64();
    let mut dt: Vec<f64> = frames.windows(2).map(|w| (w[1] - w[0]) * 1000.0).collect();
    dt.sort_by(f64::total_cmp);
    let q = |p: f64| dt.get(((dt.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(0.0);
    println!("{polls} polls in {el:.1} s ({:.0} Hz)", polls as f64 / el);
    println!("{} position changes: {:.1} fps", frames.len(), frames.len() as f64 / el);
    println!("frame interval ms: min {:.2}  p10 {:.2}  median {:.2}  p90 {:.2}  max {:.2}", q(0.0), q(0.1), q(0.5), q(0.9), q(1.0));
    println!("race clock steps: {clock_steps}; same poll as position {together}, position only {pos_only}, clock only {clock_only}");
    // which comes first within a frame: the gap from each clock step to the next position change
    let mut lag: Vec<f64> = ticks.iter().filter_map(|&c| frames.iter().find(|&&f| f >= c).map(|f| (f - c) * 1000.0)).collect();
    lag.sort_by(f64::total_cmp);
    let ql = |p: f64| lag.get(((lag.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(0.0);
    println!("clock step -> next position change, ms: min {:.3}  median {:.3}  max {:.3}", ql(0.0), ql(0.5), ql(1.0));
}
