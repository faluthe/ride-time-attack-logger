//! Unreal tagged properties, as stored in save games (GVAS) and cooked assets (.uexp).
//!
//! Both formats write the same tag layout; they differ only in how names are stored: saves use
//! inline `FString`s, cooked assets use indices into the package's name map. Values become
//! `serde_json::Value` so callers can pick fields by name.

use crate::reader::{err, Reader, Result};
use serde_json::{json, Map, Value};

/// How a format stores names.
pub trait Names {
    fn name(&self, r: &mut Reader) -> Result<String>;
    /// Object references: a path string in saves, a package index in assets.
    fn object(&self, r: &mut Reader) -> Result<Value>;
    /// Property payload size and array index (two i32s in both formats).
    fn size_and_index(&self, r: &mut Reader) -> Result<(usize, i32)> {
        let (size, index) = (r.i32()?, r.i32()?);
        Ok((size as usize, index))
    }
}

/// Save games: names are inline strings.
pub struct Inline;

impl Names for Inline {
    fn name(&self, r: &mut Reader) -> Result<String> {
        r.fstring()
    }
    fn object(&self, r: &mut Reader) -> Result<Value> {
        Ok(json!(r.fstring()?))
    }
}

/// Cooked assets: names are (index, number) into the package name map.
pub struct NameMap(pub Vec<String>);

impl Names for NameMap {
    fn name(&self, r: &mut Reader) -> Result<String> {
        let (i, n) = (r.i32()?, r.i32()?);
        let Some(base) = self.0.get(i as usize) else { return err(format!("name index {i} out of range")) };
        Ok(if n == 0 { base.clone() } else { format!("{base}_{}", n - 1) })
    }
    fn object(&self, r: &mut Reader) -> Result<Value> {
        Ok(json!(r.i32()?))
    }
}

/// Structs serialized as raw fields instead of tagged properties.
fn native(r: &mut Reader, name: &str) -> Result<Option<Value>> {
    let floats = |r: &mut Reader, n: usize| -> Result<Value> {
        let v: Vec<f32> = (0..n).map(|_| r.f32()).collect::<Result<_>>()?;
        Ok(json!(v))
    };
    Ok(Some(match name {
        "Vector" | "Rotator" => floats(r, 3)?,
        "Vector2D" => floats(r, 2)?,
        "Quat" | "LinearColor" => floats(r, 4)?,
        "Color" => json!(r.take(4)?),
        "IntPoint" => json!([r.i32()?, r.i32()?]),
        "DateTime" | "Timespan" => json!(r.i64()?),
        "Guid" => json!(r.take(16)?.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        _ => return Ok(None),
    }))
}

pub struct Props<'n, N: Names> {
    pub names: &'n N,
}

impl<N: Names> Props<'_, N> {
    /// A property list, ending at a property named "None" (or at `end`).
    pub fn list(&self, r: &mut Reader, end: Option<usize>) -> Result<Value> {
        let mut out = Map::new();
        while end.map_or(true, |e| r.p < e) {
            let name = self.names.name(r)?;
            if name == "None" {
                break;
            }
            let typ = self.names.name(r)?;
            let (size, index) = self.names.size_and_index(r)?;
            let v = self.prop(r, &typ, size)?;
            let mut key = if index != 0 { format!("{name}[{index}]") } else { name };
            if out.contains_key(&key) {
                let base = key.clone();
                key = (1..).map(|k| format!("{base}#{k}")).find(|k| !out.contains_key(k)).unwrap();
            }
            out.insert(key, v);
        }
        Ok(Value::Object(out))
    }

    fn struct_body(&self, r: &mut Reader, sname: &str, end: Option<usize>) -> Result<Value> {
        match native(r, sname)? {
            Some(v) => Ok(v),
            None => self.list(r, end),
        }
    }

    /// An element of an array/map/set: a value without a property tag.
    fn element(&self, r: &mut Reader, typ: &str, end: usize, ctx: Option<&str>) -> Result<Value> {
        Ok(match typ {
            "IntProperty" => json!(r.i32()?),
            "UInt32Property" => json!(r.u32()?),
            "Int64Property" => json!(r.i64()?),
            "FloatProperty" => json!(r.f32()?),
            "DoubleProperty" => json!(r.f64()?),
            "BoolProperty" => json!(r.u8()? != 0),
            "NameProperty" | "EnumProperty" => json!(self.names.name(r)?),
            "StrProperty" => json!(r.fstring()?),
            "SoftObjectProperty" => {
                let v = r.fstring()?;
                r.fstring()?;
                json!(v)
            }
            "ObjectProperty" => self.names.object(r)?,
            "ByteProperty" => match ctx {
                None | Some("None") => json!(r.u8()?),
                Some(_) => json!(self.names.name(r)?),
            },
            "StructProperty" => {
                if let Some(v) = ctx.map(|c| native(r, c)).transpose()?.flatten() {
                    return Ok(v);
                }
                // a struct key/value in a map has no type info: try a property list, else a Guid
                let save = r.p;
                match self.list(r, Some(end)) {
                    Ok(v) => v,
                    Err(_) => {
                        r.p = save;
                        json!(r.take(16)?.iter().map(|b| format!("{b:02x}")).collect::<String>())
                    }
                }
            }
            _ => return err(format!("unknown element type {typ}")),
        })
    }

    /// One tagged property's value; `size` is the payload size after the tag.
    pub fn prop(&self, r: &mut Reader, typ: &str, size: usize) -> Result<Value> {
        match typ {
            "BoolProperty" => {
                let v = r.u8()? != 0;
                r.u8()?;
                Ok(json!(v))
            }
            "StructProperty" => {
                let sname = self.names.name(r)?;
                r.take(17)?; // struct guid + property guid flag
                let end = r.p + size;
                let v = self.struct_body(r, &sname, Some(end)).unwrap_or_else(|e| json!({"__raw__": sname, "err": e.0}));
                r.p = end;
                Ok(v)
            }
            "ArrayProperty" => {
                let inner = self.names.name(r)?;
                r.u8()?;
                let end = r.p + size;
                let v = self.array(r, &inner, size, end).unwrap_or_else(|e| json!({"__raw__": inner, "err": e.0}));
                r.p = end;
                Ok(v)
            }
            "MapProperty" | "SetProperty" => {
                let kt = self.names.name(r)?;
                let vt = if typ == "MapProperty" { Some(self.names.name(r)?) } else { None };
                r.u8()?;
                let end = r.p + size;
                let v = (|| -> Result<Value> {
                    r.i32()?; // removed entries
                    let n = r.i32()?;
                    let mut out = vec![];
                    for _ in 0..n {
                        let k = self.element(r, &kt, end, None)?;
                        out.push(match &vt {
                            Some(vt) => json!([k, self.element(r, vt, end, None)?]),
                            None => k,
                        });
                    }
                    Ok(json!(out))
                })()
                .unwrap_or_else(|e| json!({"__raw__": [kt, vt], "err": e.0}));
                r.p = end;
                Ok(v)
            }
            "EnumProperty" => {
                self.names.name(r)?; // enum type
                r.u8()?;
                Ok(json!(self.names.name(r)?))
            }
            "ByteProperty" => {
                let en = self.names.name(r)?;
                r.u8()?;
                Ok(if en == "None" { json!(r.u8()?) } else { json!(self.names.name(r)?) })
            }
            _ => {
                r.u8()?; // property guid flag
                let end = r.p + size;
                let v = if typ == "TextProperty" { Value::Null } else { self.element(r, typ, end, None).unwrap_or(json!({"__raw__": typ})) };
                r.p = end;
                Ok(v)
            }
        }
    }

    fn array(&self, r: &mut Reader, inner: &str, size: usize, end: usize) -> Result<Value> {
        let n = r.i32()?;
        if inner == "StructProperty" {
            self.names.name(r)?; // repeated property name
            self.names.name(r)?; // "StructProperty"
            r.i64()?;
            let sname = self.names.name(r)?;
            r.take(17)?;
            let v: Vec<Value> = (0..n).map(|_| self.struct_body(r, &sname, Some(end))).collect::<Result<_>>()?;
            return Ok(json!(v));
        }
        if inner == "ByteProperty" && size >= 4 && size - 4 == n as usize {
            return Ok(json!(r.take(n as usize)?)); // raw byte blob
        }
        let v: Vec<Value> = (0..n).map(|_| self.element(r, inner, end, None)).collect::<Result<_>>()?;
        Ok(json!(v))
    }
}
