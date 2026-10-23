//! One telemetry frame.

/// A logged frame. Field order matches the CSV columns.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Seconds since the session started.
    pub t: f64,
    /// World position, metres.
    pub pos: [f32; 3],
    /// Forward unit vector.
    pub fwd: [f32; 3],
    /// z of the bike's side and up axes (lean comes from `side_z`).
    pub side_z: f32,
    pub up_z: f32,
    /// Rider inputs 0..1; `None` for sessions recorded before inputs were found.
    pub inputs: Option<Inputs>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Inputs {
    pub throttle: f32,
    pub brake: f32,
    /// Rear brake button, 0 or 1.
    pub rear: f32,
    /// 0 = neutral.
    pub gear: i32,
    /// 0 = idle .. 1 = limiter.
    pub rpm: f32,
}

impl Sample {
    /// Build from a rotation quaternion (x, y, z, w).
    pub fn from_quat(t: f64, pos: [f32; 3], q: [f32; 4], inputs: Option<Inputs>) -> Self {
        let [x, y, z, w] = q;
        Self {
            t,
            pos,
            fwd: [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)],
            side_z: 2.0 * (y * z + w * x),
            up_z: 1.0 - 2.0 * (x * x + y * y),
            inputs,
        }
    }

    /// Lean angle in degrees, right positive.
    pub fn lean_deg(&self) -> f32 {
        -self.side_z.clamp(-1.0, 1.0).asin().to_degrees()
    }

    /// Heading in degrees, from the forward vector (0 = +x, counter-clockwise).
    pub fn heading_deg(&self) -> f32 {
        self.fwd[1].atan2(self.fwd[0]).to_degrees()
    }
}
