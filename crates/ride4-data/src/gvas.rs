//! Save games (GVAS). Ride 4 writes them with an `mgvs` magic but the standard UE 4.25 layout.

use crate::props::{Inline, Props};
use crate::reader::{Reader, Result};
use serde_json::Value;

/// The save's properties, plus `__class__` (the save game class path).
pub fn load(bytes: &[u8]) -> Result<Value> {
    let mut r = Reader::new(bytes);
    r.take(4)?; // magic
    r.i32()?; // save game version
    r.i32()?; // package version
    r.take(6)?; // engine major, minor, patch
    r.i32()?; // engine build
    r.fstring()?; // engine branch
    r.i32()?; // custom version format
    let custom = r.i32()?;
    r.take(20 * custom.max(0) as usize)?;
    let class = r.fstring()?;
    let mut v = Props { names: &Inline }.list(&mut r, None)?;
    v.as_object_mut().unwrap().insert("__class__".into(), Value::String(class));
    Ok(v)
}
