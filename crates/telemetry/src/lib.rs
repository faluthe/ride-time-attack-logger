//! Telemetry data model and processing. No UI or OS-specific code.

pub mod csv;
pub mod sample;

pub use sample::{Inputs, Sample};
