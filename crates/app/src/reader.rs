//! Background reader: attaches to the game, samples the bike at a fixed rate, records sessions,
//! and publishes a snapshot the UI can copy each frame.

use ride4_mem::{LinuxProcess, Session, EXE};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use telemetry::csv::SessionWriter;
use telemetry::{Inputs, Sample};

pub const HZ: f64 = 50.0;
/// Seconds of history kept for the live graphs.
pub const HISTORY_S: f64 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    /// Ride 4 isn't running.
    NoGame,
    /// Process found but the reader couldn't attach yet (game still starting, or permissions).
    Attaching(String),
    /// Attached, no bike: menus or loading.
    NoBike,
    /// Reading the player's bike.
    Riding,
}

/// One point of graph history.
#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub t: f64,
    pub speed: f32,
    pub lean: f32,
    pub inputs: Inputs,
}

#[derive(Clone, Debug)]
pub struct Recording {
    pub path: PathBuf,
    pub rows: usize,
    pub started: Instant,
}

/// Everything the UI needs, copied out under the lock once per frame.
#[derive(Clone, Debug)]
pub struct Live {
    pub link: Link,
    pub pid: Option<u32>,
    pub bike: Option<u64>,
    pub controller: Option<String>,
    pub sample: Option<Sample>,
    /// km/h: position change over the last ~0.5 s, lightly smoothed (reader ticks and game frames
    /// aren't aligned, so short windows alias into jitter).
    pub speed: f32,
    pub history: VecDeque<Point>,
    pub recording: Option<Recording>,
    /// Actual sample rate over the last second.
    pub rate: f32,
}

impl Default for Live {
    fn default() -> Self {
        Self { link: Link::NoGame, pid: None, bike: None, controller: None, sample: None, speed: 0.0,
            history: VecDeque::new(), recording: None, rate: 0.0 }
    }
}

pub struct Reader {
    pub live: Arc<Mutex<Live>>,
    /// Record sessions to disk while a bike is on track.
    pub record: Arc<AtomicBool>,
    pub sessions_dir: PathBuf,
}

impl Reader {
    pub fn spawn(sessions_dir: PathBuf, record: bool) -> Self {
        let live = Arc::new(Mutex::new(Live::default()));
        let rec = Arc::new(AtomicBool::new(record));
        let (l, r, dir) = (live.clone(), rec.clone(), sessions_dir.clone());
        thread::Builder::new().name("reader".into()).spawn(move || run(l, r, dir)).expect("spawn reader");
        Self { live, record: rec, sessions_dir }
    }

    pub fn snapshot(&self) -> Live {
        self.live.lock().unwrap().clone()
    }
}

fn set(live: &Mutex<Live>, f: impl FnOnce(&mut Live)) {
    f(&mut live.lock().unwrap());
}

/// Local time as `YYYYMMDD-HHMMSS`, matching the old logger's session names.
fn stamp() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn run(live: Arc<Mutex<Live>>, record: Arc<AtomicBool>, dir: PathBuf) {
    loop {
        let Some(pid) = LinuxProcess::find(EXE) else {
            set(&live, |l| *l = Live::default());
            thread::sleep(Duration::from_secs(2));
            continue;
        };
        let mut session = match Session::attach(pid) {
            Ok(s) => s,
            Err(e) => {
                set(&live, |l| { l.link = Link::Attaching(e.to_string()); l.pid = Some(pid); });
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        set(&live, |l| { l.link = Link::NoBike; l.pid = Some(pid); });
        ride(&mut session, &live, &record, &dir);
    }
}

/// Sample until the game exits.
fn ride(session: &mut Session, live: &Mutex<Live>, record: &AtomicBool, dir: &std::path::Path) {
    let t0 = Instant::now();
    let period = Duration::from_secs_f64(1.0 / HZ);
    let mut next = Instant::now();
    let mut writer: Option<(SessionWriter, PathBuf, Instant, f64)> = None;
    let mut recent: VecDeque<(f64, [f32; 3])> = VecDeque::new();
    let mut last_pos = None;
    let mut bike = None;
    let mut ticks: VecDeque<Instant> = VecDeque::new();

    while session.process.alive() {
        let state = session.poll();
        let now = Instant::now();
        let t = (now - t0).as_secs_f64();
        ticks.push_back(now);
        while ticks.front().is_some_and(|&x| now - x > Duration::from_secs(1)) {
            ticks.pop_front();
        }

        let actor = matches!(session.status(), ride4_mem::Status::Riding { .. }).then(|| session.chain().map(|c| c.bike)).flatten();
        if actor != bike {
            // New pawn (restart, new event) or back to the menu: close the current recording.
            if let Some((w, ..)) = writer.take() {
                let _ = w.finish();
            }
            bike = actor;
            recent.clear();
            last_pos = None;
            let controller = actor.and_then(|_| session.chain()).and_then(|c| session.class_of(c.controller));
            set(live, |l| { l.history.clear(); l.bike = actor; l.controller = controller; l.recording = None; });
        }

        match state {
            None => set(live, |l| { l.link = Link::NoBike; l.sample = None; l.speed = 0.0; l.rate = ticks.len() as f32; }),
            Some(b) => {
                let inputs = Inputs { throttle: b.throttle, brake: b.brake, rear: b.rear, gear: b.gear, rpm: b.rpm };
                // The game only moves the bike once per rendered frame; skip repeats so speed and
                // recordings aren't polluted by duplicate positions (pauses, sampling faster than fps).
                let moved = last_pos != Some(b.pos);
                last_pos = Some(b.pos);

                if record.load(Ordering::Relaxed) {
                    if writer.is_none() {
                        let path = dir.join(format!("{}.csv", stamp()));
                        match SessionWriter::create(&path) {
                            Ok(w) => writer = Some((w, path, now, t)),
                            Err(e) => eprintln!("can't record to {}: {e}", path.display()),
                        }
                    }
                } else if let Some((w, ..)) = writer.take() {
                    let _ = w.finish();
                }

                let rec_t0 = writer.as_ref().map_or(t, |w| w.3);
                let sample = Sample::from_quat(t - rec_t0, b.pos, b.rot, Some(inputs));
                if moved {
                    if let Some((w, ..)) = writer.as_mut() {
                        let _ = w.write(&sample);
                    }
                    recent.push_back((t, b.pos));
                    while recent.len() > 2 && t - recent[1].0 >= 0.5 {
                        recent.pop_front();
                    }
                }
                let speed = match (recent.front(), recent.back()) {
                    (Some(a), Some(z)) if z.0 - a.0 > 0.05 => {
                        let d = ((z.1[0] - a.1[0]).powi(2) + (z.1[1] - a.1[1]).powi(2) + (z.1[2] - a.1[2]).powi(2)).sqrt();
                        (d as f64 / (z.0 - a.0) * 3.6) as f32
                    }
                    _ => 0.0,
                };
                let recording = writer.as_ref().map(|(w, p, st, _)| Recording { path: p.clone(), rows: w.rows(), started: *st });
                set(live, |l| {
                    l.link = Link::Riding;
                    if moved {
                        l.speed = if l.speed == 0.0 { speed } else { l.speed + 0.3 * (speed - l.speed) };
                    }
                    l.sample = Some(sample);
                    l.history.push_back(Point { t, speed: l.speed, lean: sample.lean_deg(), inputs });
                    while l.history.front().is_some_and(|p| t - p.t > HISTORY_S) {
                        l.history.pop_front();
                    }
                    l.recording = recording;
                    l.rate = ticks.len() as f32;
                });
            }
        }
        next += period;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    if let Some((w, ..)) = writer.take() {
        let _ = w.finish();
    }
    set(live, |l| *l = Live::default());
}
