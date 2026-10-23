//! Compact binary lap traces, one file per lap. `rta-export` gzips them into
//! `laps/<id>.bin.gz`; the README ("Lap files") documents the format for the site's decoder.
//!
//! Every channel is quantised to the decimals the trace is already rounded to, so decoding gives
//! back exactly the values the JSON used to carry. Values are stored as the first one plus deltas,
//! each channel with the narrowest signed width (1, 2 or 4 bytes) that holds all of its deltas.

use crate::laps::Trace;

pub const MAGIC: &[u8; 4] = b"RTAL";
pub const VERSION: u16 = 1;
/// Bytes before the channel table.
const HEAD: usize = 12;
/// Bytes per channel table entry.
const ENTRY: usize = 12;

/// Channel name -> decimals kept, in file order. Input channels are written only when present.
pub const CHANNELS: [(&str, u8); 12] = [
    ("t", 3),
    ("d", 1),
    ("x", 2),
    ("y", 2),
    ("v", 1),
    ("lean", 1),
    ("ax", 2),
    ("thr", 2),
    ("brk", 2),
    ("rear", 0),
    ("gear", 0),
    ("rpm", 3),
];

fn quantise(v: &[f64], decimals: u8) -> Vec<i32> {
    let m = 10f64.powi(decimals as i32);
    v.iter().map(|x| (x * m).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32).collect()
}

fn pad4(b: &mut Vec<u8>) {
    while b.len() % 4 != 0 {
        b.push(0);
    }
}

/// Encode a trace. All channels must have the same length.
pub fn encode(tr: &Trace) -> Vec<u8> {
    let f64s = |v: &[i32]| v.iter().map(|&x| x as f64).collect::<Vec<_>>();
    let rear = tr.rear.as_ref().map(|v| v.iter().map(|&x| x as f64).collect::<Vec<_>>());
    let gear = tr.gear.as_ref().map(|v| f64s(v));
    let cols: Vec<Option<&Vec<f64>>> = vec![
        Some(&tr.t), Some(&tr.d), Some(&tr.x), Some(&tr.y), Some(&tr.v), Some(&tr.lean), Some(&tr.ax),
        tr.thr.as_ref(), tr.brk.as_ref(), rear.as_ref(), gear.as_ref(), tr.rpm.as_ref(),
    ];
    let n = tr.t.len();
    let chans: Vec<(&str, u8, Vec<i32>)> = CHANNELS
        .iter()
        .zip(cols)
        .filter_map(|(&(name, dec), c)| c.map(|v| (name, dec, quantise(v, dec))))
        .collect();
    assert!(chans.iter().all(|c| c.2.len() == n), "trace channels differ in length");

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(chans.len() as u16).to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    let mut data = Vec::new();
    for (name, dec, q) in &chans {
        let deltas: Vec<i64> = q.windows(2).map(|w| w[1] as i64 - w[0] as i64).collect();
        let width: u8 = if deltas.iter().all(|&d| i8::try_from(d).is_ok()) {
            1
        } else if deltas.iter().all(|&d| i16::try_from(d).is_ok()) {
            2
        } else {
            4
        };
        let mut id = [0u8; 4];
        id[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&id);
        out.push(width);
        out.push(*dec);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&q.first().copied().unwrap_or(0).to_le_bytes());
        for d in deltas {
            match width {
                1 => data.push(d as i8 as u8),
                2 => data.extend_from_slice(&(d as i16).to_le_bytes()),
                // quantised values are i32, but a delta between extremes can overflow: wrapping
                // arithmetic on decode undoes the wrap
                _ => data.extend_from_slice(&(d as i32).to_le_bytes()),
            }
        }
        pad4(&mut data);
    }
    out.extend_from_slice(&data);
    out
}

/// Decode a lap file back into a trace. Unknown channels are skipped.
pub fn decode(b: &[u8]) -> Result<Trace, String> {
    let get = |at: usize, len: usize| b.get(at..at + len).ok_or_else(|| "truncated lap file".to_string());
    if get(0, 4)? != MAGIC {
        return Err("not a lap file".into());
    }
    let u16_at = |at| get(at, 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
    let u32_at = |at| get(at, 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let version = u16_at(4)?;
    if version != VERSION {
        return Err(format!("unsupported lap file version {version}"));
    }
    let count = u16_at(6)? as usize;
    let n = u32_at(8)? as usize;
    let mut tr = Trace::default();
    let mut pos = HEAD + count * ENTRY;
    for c in 0..count {
        let e = get(HEAD + c * ENTRY, ENTRY)?;
        let name = std::str::from_utf8(&e[..4]).map_err(|_| "bad channel name")?.trim_end_matches('\0');
        let (width, dec) = (e[4] as usize, e[5] as i32);
        if ![1, 2, 4].contains(&width) {
            return Err(format!("bad width {width} for channel {name}"));
        }
        let first = i32::from_le_bytes([e[8], e[9], e[10], e[11]]);
        let len = n.saturating_sub(1) * width;
        let raw = get(pos, len)?;
        pos += len.div_ceil(4) * 4;
        let mut q = Vec::with_capacity(n);
        if n > 0 {
            q.push(first);
            for d in raw.chunks_exact(width) {
                let d = match width {
                    1 => d[0] as i8 as i32,
                    2 => i16::from_le_bytes([d[0], d[1]]) as i32,
                    _ => i32::from_le_bytes([d[0], d[1], d[2], d[3]]),
                };
                q.push(q.last().unwrap().wrapping_add(d));
            }
        }
        let m = 10f64.powi(dec);
        let vals: Vec<f64> = q.iter().map(|&x| x as f64 / m).collect();
        match name {
            "t" => tr.t = vals,
            "d" => tr.d = vals,
            "x" => tr.x = vals,
            "y" => tr.y = vals,
            "v" => tr.v = vals,
            "lean" => tr.lean = vals,
            "ax" => tr.ax = vals,
            "thr" => tr.thr = Some(vals),
            "brk" => tr.brk = Some(vals),
            "rear" => tr.rear = Some(q.iter().map(|&x| x as u8).collect()),
            "gear" => tr.gear = Some(q),
            "rpm" => tr.rpm = Some(vals),
            _ => {}
        }
    }
    Ok(tr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: usize, inputs: bool) -> Trace {
        let r = |v: f64, p: i32| (v * 10f64.powi(p)).round() / 10f64.powi(p);
        let mut tr = Trace::default();
        for i in 0..n {
            let f = i as f64;
            tr.t.push(r(f * 0.04 + 0.0013, 3));
            tr.d.push(r(f * 2.7 - 3.0, 1));
            tr.x.push(r(-1523.456 + (f * 0.05).sin() * 300.0, 2));
            tr.y.push(r(8123.4 + (f * 0.05).cos() * 300.0, 2));
            tr.v.push(r(150.0 + (f * 0.1).sin() * 90.0, 1));
            tr.lean.push(r((f * 0.07).sin() * 55.0, 1));
            tr.ax.push(r((f * 0.13).cos() * 1.4, 2));
        }
        if inputs {
            tr.thr = Some((0..n).map(|i| r((i % 37) as f64 / 36.0, 2)).collect());
            tr.brk = Some((0..n).map(|i| if i % 50 < 10 { 1.0 } else { 0.0 }).collect());
            tr.rear = Some((0..n).map(|i| (i % 90 < 3) as u8).collect());
            tr.gear = Some((0..n).map(|i| 1 + (i / 40 % 6) as i32).collect());
            tr.rpm = Some((0..n).map(|i| r((i % 40) as f64 / 40.0 + 0.2, 3)).collect());
        }
        tr
    }

    fn same(a: &Trace, b: &Trace) {
        assert_eq!(a, b);
    }

    #[test]
    fn round_trip_with_inputs() {
        let tr = sample(3000, true);
        let b = encode(&tr);
        same(&decode(&b).unwrap(), &tr);
        assert_eq!(b.len() % 4, 0);
    }

    #[test]
    fn round_trip_without_inputs() {
        let tr = sample(500, false);
        let back = decode(&encode(&tr)).unwrap();
        same(&back, &tr);
        assert!(back.thr.is_none() && back.gear.is_none());
    }

    #[test]
    fn empty_and_single() {
        for n in [0, 1, 2] {
            let tr = sample(n, true);
            same(&decode(&encode(&tr)).unwrap(), &tr);
        }
    }

    #[test]
    fn widens_for_big_jumps() {
        let mut tr = sample(200, true);
        tr.x[100] += 5000.0; // a reset: 500 000 cm
        tr.t[150..].iter_mut().for_each(|t| *t += 90.0); // a pause
        tr.v[10] = 0.0;
        let b = encode(&tr);
        same(&decode(&b).unwrap(), &tr);
        let width = |name: &str| {
            let c = CHANNELS.iter().position(|c| c.0 == name).unwrap();
            b[HEAD + c * ENTRY + 4]
        };
        assert_eq!((width("x"), width("t"), width("v"), width("gear")), (4, 4, 2, 1));
    }

    #[test]
    fn extreme_values_wrap_back() {
        let mut tr = sample(3, false);
        tr.x = vec![-21_000_000.0, 21_000_000.0, -21_000_000.0];
        same(&decode(&encode(&tr)).unwrap(), &tr);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"nope").is_err());
        let b = encode(&sample(100, true));
        assert!(decode(&b[..b.len() - 40]).is_err());
    }
}
