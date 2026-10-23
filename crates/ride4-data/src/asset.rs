//! UE 4.25 cooked packages: the name map from `.uasset`, tagged properties from `.uexp`.

use crate::props::{NameMap, Props};
use crate::reader::{Reader, Result};
use serde_json::{Map, Value};

/// The package name map (from the `.uasset` summary).
pub fn name_map(uasset: &[u8]) -> Result<NameMap> {
    let mut r = Reader::new(uasset);
    r.u32()?; // tag
    let legacy = r.i32()?;
    if legacy != -4 {
        r.i32()?; // legacy UE3 version
    }
    r.i32()?; // UE4 version
    r.i32()?; // licensee version
    let custom = r.i32()?;
    r.take(20 * custom.max(0) as usize)?; // custom versions
    r.i32()?; // total header size
    r.fstring()?; // folder name
    r.u32()?; // package flags
    let (count, offset) = (r.i32()?, r.i32()?);
    r.p = offset as usize;
    let mut names = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count {
        names.push(r.fstring()?);
        r.take(4)?; // hashes
    }
    Ok(NameMap(names))
}

/// Rows of a cooked `DataTable` by row name.
pub fn datatable(uasset: &[u8], uexp: &[u8]) -> Result<Map<String, Value>> {
    let names = name_map(uasset)?;
    let props = Props { names: &names };
    let mut r = Reader::new(uexp);
    props.list(&mut r, None)?; // the table object's own properties (RowStruct etc.)
    r.i32()?; // object guid flag
    let mut rows = Map::new();
    for _ in 0..r.i32()? {
        let key = crate::props::Names::name(&names, &mut r)?;
        rows.insert(key, props.list(&mut r, None)?);
    }
    Ok(rows)
}
