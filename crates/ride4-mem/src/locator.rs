//! Follow Unreal's object graph from GWorld to the player's bike.

use crate::names::Names;
use crate::offsets::*;
use crate::process::{is_ptr, Memory};

pub const BIKE_CLASS: &str = "BaseMotorBike_C";

/// Objects found at the end of the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chain {
    pub world: u64,
    pub controller: u64,
    pub bike: u64,
}

/// Walk the chain from one GWorld candidate. `None` in menus and while loading (no pawn).
pub fn resolve(m: &impl Memory, gworld: u64) -> Option<Chain> {
    let deref = |p: u64| m.u64(p).filter(|&v| is_ptr(v));
    let world = deref(gworld)?;
    let gi = deref(world + WORLD_GAME_INSTANCE)?;
    let players = deref(gi + GI_LOCAL_PLAYERS)?;
    let local = deref(players)?;
    let controller = deref(local + LP_PLAYER_CONTROLLER)?;
    let bike = deref(controller + PC_PAWN)?;
    Some(Chain { world, controller, bike })
}

/// Class name of a UObject.
pub fn class_name(m: &impl Memory, names: &Names, obj: u64) -> Option<String> {
    let class = m.u64(obj + UOBJECT_CLASS).filter(|&c| is_ptr(c))?;
    names.get(m, m.i32(class + UOBJECT_NAME)?)
}

fn leads_to_bike(m: &impl Memory, names: &Names, g: u64) -> bool {
    resolve(m, g).is_some_and(|c| class_name(m, names, c.bike).as_deref() == Some(BIKE_CLASS))
}

/// The known GWorld address that currently leads to a bike. Cheap: a handful of reads.
pub fn find_gworld(m: &impl Memory, names: &Names) -> Option<u64> {
    GWORLD.into_iter().find(|&g| leads_to_bike(m, names, g))
}

/// Slow fallback for when a game update moved GWorld: try every pointer-sized slot in `.data`.
pub fn scan_gworld(m: &impl Memory, names: &Names) -> Option<u64> {
    (DATA.0..DATA.1).step_by(8).find(|&g| leads_to_bike(m, names, g))
}

/// Instantaneous state of one bike, in game units converted to metres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BikeState {
    /// World position, metres (Unreal axes: x forward on the map, z up).
    pub pos: [f32; 3],
    /// Rotation quaternion (x, y, z, w).
    pub rot: [f32; 4],
    /// Rider inputs, 0..1. `rear` is a button, so 0 or 1.
    pub throttle: f32,
    pub brake: f32,
    pub rear: f32,
    /// 0 = neutral.
    pub gear: i32,
    /// Engine speed, 0 = idle .. 1 = limiter.
    pub rpm: f32,
}

/// Component pointers for one bike actor. Rebuild when the pawn changes.
#[derive(Clone, Copy, Debug)]
pub struct Bike {
    pub actor: u64,
    pub mesh: u64,
    pub movement: u64,
    pub ignition: u64,
    pub audio: u64,
}

impl Bike {
    pub fn new(m: &impl Memory, actor: u64) -> Option<Self> {
        let get = |off| m.u64(actor + off).filter(|&p| is_ptr(p));
        Some(Self {
            actor,
            mesh: get(BIKE_MESH)?,
            movement: get(BIKE_MOVEMENT)?,
            ignition: get(BIKE_IGNITION)?,
            audio: get(BIKE_AUDIO)?,
        })
    }

    /// One read of every channel. `None` if the actor was freed mid-read.
    pub fn sample(&self, m: &impl Memory) -> Option<BikeState> {
        let mut t = [0u8; 28];
        let mut inp = [0u8; 12];
        if !m.read(self.mesh + MESH_ROTATION, &mut t) || !m.read(self.movement + MOVE_THROTTLE, &mut inp) {
            return None;
        }
        let f = |b: &[u8], i: usize| f32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
        Some(BikeState {
            rot: [f(&t, 0), f(&t, 1), f(&t, 2), f(&t, 3)],
            pos: [f(&t, 4) / 100.0, f(&t, 5) / 100.0, f(&t, 6) / 100.0],
            throttle: f(&inp, 0),
            brake: f(&inp, 1),
            rear: f(&inp, 2),
            gear: m.i32(self.ignition + IGNITION_GEAR)?,
            rpm: m.f32(self.audio + AUDIO_RPM)?,
        })
    }
}
