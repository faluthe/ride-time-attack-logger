//! Read Ride 4's game files (paks, cooked assets) and save games (GVAS).

pub mod asset;
pub mod gvas;
pub mod pak;
pub mod props;
pub mod reader;

pub use pak::Game;
pub use reader::{Error, Result};
