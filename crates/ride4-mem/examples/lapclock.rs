//! Find the current-lap timer near the objects we already reach: the bike and its components,
//! the controller, the game state, `HudLogicComponent`, the results manager and the player's
//! result item, plus every object those point to directly.
//! `cargo run --release -p ride4-mem --example lapclock`
//!
//! Prints f32/f64 seconds and i32 milliseconds that advance at wall-clock rate, next to the race
//! clock, so a lap timer shows up as `race clock - lap start`.

use ride4_mem::offsets::*;
use ride4_mem::{LinuxProcess, Memory, Session, EXE};
use std::time::{Duration, Instant};

const LEN: usize = 0x1400;

fn is_ptr(v: u64) -> bool {
    (0x10000..0x7fff_ffff_ffff).contains(&v) && v.is_multiple_of(8)
}

fn read(m: &LinuxProcess, a: u64) -> Option<Vec<u8>> {
    let mut b = vec![0u8; LEN];
    m.read(a, &mut b).then_some(b)
}

/// (kind, offset, value in seconds) for every plausible clock field.
fn fields(b: &[u8]) -> Vec<(&'static str, usize, f64)> {
    let mut out = vec![];
    for o in (0..b.len() - 8).step_by(4) {
        let w4: [u8; 4] = b[o..o + 4].try_into().unwrap();
        out.push(("f32", o, f32::from_le_bytes(w4) as f64));
        out.push(("ms", o, i32::from_le_bytes(w4) as f64 / 1000.0));
        if o % 8 == 0 {
            out.push(("f64", o, f64::from_le_bytes(b[o..o + 8].try_into().unwrap())));
        }
    }
    out
}

fn main() {
    let pid = LinuxProcess::find(EXE).expect("Ride 4 is not running");
    let mut s = Session::attach(pid).expect("attach");
    for _ in 0..50 {
        if s.poll().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let m = &s.process;
    let c = s.chain().expect("no bike: get on track");
    let d = |p: u64| m.u64(p).filter(|&v| is_ptr(v));

    let mut roots: Vec<(String, u64)> = vec![("bike".into(), c.bike), ("controller".into(), c.controller)];
    for (name, off) in [("mesh", BIKE_MESH), ("movement", BIKE_MOVEMENT), ("ignition", BIKE_IGNITION), ("audio", BIKE_AUDIO)] {
        roots.extend(d(c.bike + off).map(|p| (name.into(), p)));
    }
    if let Some(gs) = d(c.world + WORLD_GAME_STATE) {
        roots.push(("gamestate".into(), gs));
        if let Some(hud) = d(gs + GAME_STATE_HUD_LOGIC) {
            roots.push(("hud".into(), hud));
            if let Some(res) = d(hud + HUD_RESULTS) {
                roots.push(("results".into(), res));
                if let Some(item) = d(res + RESULTS_ITEMS).and_then(d) {
                    roots.push(("item".into(), item));
                }
            }
        }
    }
    // one level further from each root
    let mut objs = roots.clone();
    for (name, a) in &roots {
        let Some(b) = read(m, *a) else { continue };
        for o in (0..0x400).step_by(8) {
            let p = u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
            if is_ptr(p) && !objs.iter().any(|x| x.1 == p) {
                objs.push((format!("{name}+{o:#x}"), p));
            }
        }
    }
    println!("{} objects", objs.len());

    let clock = || s.timing().map(|t| t.race_clock as f64).unwrap_or(f64::NAN);
    let snap = || objs.iter().map(|(_, a)| read(m, *a).map(|b| fields(&b))).collect::<Vec<_>>();
    let (t0, c0, s0) = (Instant::now(), clock(), snap());
    std::thread::sleep(Duration::from_millis(1500));
    let (t1, s1) = (Instant::now(), snap());
    std::thread::sleep(Duration::from_millis(1500));
    let (t2, c2, s2) = (Instant::now(), clock(), snap());
    let (dt1, dt2) = ((t1 - t0).as_secs_f64(), (t2 - t0).as_secs_f64());
    println!("race clock {c0:.3} -> {c2:.3} over {dt2:.3}s wall");

    let ok = |a: f64, b: f64, dt: f64| a.is_finite() && (0.0..36000.0).contains(&a) && ((b - a) - dt).abs() < 0.1 + dt * 0.03;
    for (i, (name, a)) in objs.iter().enumerate() {
        let (Some(f0), Some(f1), Some(f2)) = (&s0[i], &s1[i], &s2[i]) else { continue };
        for j in 0..f0.len() {
            let (k, o, v0) = f0[j];
            if ok(v0, f1[j].2, dt1) && ok(v0, f2[j].2, dt2) {
                println!("{name:<24} {a:#x} +{o:#06x} {k:<3} {:10.3}   race clock - this = {:.3}", f2[j].2, c2 - f2[j].2);
            }
        }
    }
}
