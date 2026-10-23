//! Telemetry data model and processing. No UI or OS-specific code.

pub mod csv;
pub mod laps;
pub mod sample;
pub mod track;

pub use sample::{Inputs, Sample};
