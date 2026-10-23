//! Locate and read the player's bike in Ride 4's memory.
//!
//! ```no_run
//! use ride4_mem::{Session, EXE};
//! let pid = ride4_mem::LinuxProcess::find(EXE).unwrap();
//! let mut s = Session::attach(pid).unwrap();
//! if let Some(state) = s.poll() { println!("{state:?}"); }
//! ```

pub mod locator;
pub mod names;
pub mod offsets;
pub mod process;

pub use locator::{Bike, BikeState, Chain};
pub use process::{LinuxProcess, Memory};

use locator::{class_name, find_gworld, resolve, scan_gworld, BIKE_CLASS};
use std::time::{Duration, Instant};

/// How often to try the slow `.data` rescan while the known GWorld addresses lead nowhere.
const RESCAN_EVERY: Duration = Duration::from_secs(10);
use names::Names;

pub const EXE: &str = "ride4-Win64-Shipping.exe";

/// Where the reader currently is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Attached, but there's no bike (menus, loading).
    NoBike,
    /// Reading this bike actor.
    Riding { bike: u64 },
}

/// An attached game process plus the cached chain. Call `poll` at the sample rate.
pub struct Session {
    pub process: LinuxProcess,
    names: Names,
    gworld: Option<u64>,
    bike: Option<Bike>,
    last_scan: Option<Instant>,
}

impl Session {
    pub fn attach(pid: u32) -> std::io::Result<Self> {
        let process = LinuxProcess::open(pid)?;
        let names = Names::find(&process)
            .ok_or_else(|| std::io::Error::other("FNamePool not found (game still starting?)"))?;
        Ok(Self { process, names, gworld: None, bike: None, last_scan: None })
    }

    pub fn status(&self) -> Status {
        self.bike.map_or(Status::NoBike, |b| Status::Riding { bike: b.actor })
    }

    /// Current chain, for display.
    pub fn chain(&self) -> Option<Chain> {
        self.gworld.and_then(|g| resolve(&self.process, g))
    }

    pub fn class_of(&self, obj: u64) -> Option<String> {
        class_name(&self.process, &self.names, obj)
    }

    /// Read the player's bike, re-locating it when the pawn changes. `None` when there's no bike.
    pub fn poll(&mut self) -> Option<BikeState> {
        let m = &self.process;
        let pawn = self.gworld.and_then(|g| resolve(m, g)).map(|c| c.bike);
        if pawn.is_none() || pawn != self.bike.map(|b| b.actor) {
            self.bike = None;
            // Cheap paths first: the cached GWorld, then the known addresses. The full .data
            // rescan only runs every RESCAN_EVERY, since in menus there is legitimately no bike.
            let mut g = match (self.gworld, pawn) {
                (Some(g), Some(_)) => Some(g),
                _ => find_gworld(m, &self.names),
            };
            if g.is_none() && self.last_scan.map_or(true, |t| t.elapsed() >= RESCAN_EVERY) {
                self.last_scan = Some(Instant::now());
                g = scan_gworld(m, &self.names);
            }
            self.gworld = g.or(self.gworld);
            let actor = g.and_then(|g| resolve(m, g)).map(|c| c.bike)?;
            if class_name(m, &self.names, actor).as_deref() != Some(BIKE_CLASS) {
                return None;
            }
            self.bike = Bike::new(m, actor);
        }
        let s = self.bike?.sample(m);
        if s.is_none() {
            self.bike = None;
        }
        s
    }
}
