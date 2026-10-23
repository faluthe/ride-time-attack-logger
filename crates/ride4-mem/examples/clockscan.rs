//! Find running clocks (e.g. the lap timer): values that advance at exactly wall-clock rate.
//!
//! ```text
//! cargo run --release -p ride4-mem --example clockscan            # scan while the timer runs
//! cargo run --release -p ride4-mem --example clockscan -- watch SECS ADDR...   # log values at 20 Hz
//! ```
//! The scan snapshots all writable memory, waits, and keeps f32/f64 seconds and i32 milliseconds
//! that grew by the elapsed time; a few more rounds drop coincidences. Survivors are printed with
//! the Unreal object they sit in, when one can be found just before them.

use ride4_mem::locator::class_name;
use ride4_mem::names::Names;
use ride4_mem::offsets::{IMAGE_BASE, UOBJECT_NAME};

use ride4_mem::{LinuxProcess, Memory, EXE};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    F32,
    F64,
    MsI32,
}

#[derive(Clone, Copy, Debug)]
struct Cand {
    addr: u64,
    kind: Kind,
}

fn regions(pid: u32) -> Vec<(u64, u64)> {
    std::fs::read_to_string(format!("/proc/{pid}/maps")).unwrap().lines().filter_map(|l| {
        let mut f = l.split_whitespace();
        let (range, perms) = (f.next()?, f.next()?);
        if !perms.starts_with("rw") {
            return None;
        }
        let (a, b) = range.split_once('-')?;
        Some((u64::from_str_radix(a, 16).ok()?, u64::from_str_radix(b, 16).ok()?))
    }).collect()
}

fn snapshot(p: &LinuxProcess, regs: &[(u64, u64)]) -> Vec<(u64, Vec<u8>)> {
    regs.iter().filter_map(|&(a, b)| {
        let mut buf = vec![0u8; (b - a) as usize];
        p.read(a, &mut buf).then_some((a, buf))
    }).collect()
}

fn value(p: &LinuxProcess, c: Cand) -> Option<f64> {
    match c.kind {
        Kind::F32 => p.f32(c.addr).map(|v| v as f64),
        Kind::F64 => { let mut b = [0; 8]; p.read(c.addr, &mut b).then(|| f64::from_le_bytes(b)) }
        Kind::MsI32 => p.i32(c.addr).map(|v| v as f64 / 1000.0),
    }
}

/// Does `a -> b` look like a clock that ran for `dt` seconds?
fn ticks(a: f64, b: f64, dt: f64) -> bool {
    a.is_finite() && b.is_finite() && (0.0..36000.0).contains(&a) && ((b - a) - dt).abs() < 0.08 + dt * 0.02
}

fn first_pass(s0: &[(u64, Vec<u8>)], s1: &[(u64, Vec<u8>)], dt: f64) -> Vec<Cand> {
    let mut out = vec![];
    for ((base, a), (_, b)) in s0.iter().zip(s1) {
        let n = a.len().min(b.len());
        for i in (0..n.saturating_sub(7)).step_by(4) {
            let f = |buf: &[u8]| f32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as f64;
            let ms = |buf: &[u8]| i32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as f64 / 1000.0;
            if ticks(f(a), f(b), dt) {
                out.push(Cand { addr: base + i as u64, kind: Kind::F32 });
            }
            if ticks(ms(a), ms(b), dt) {
                out.push(Cand { addr: base + i as u64, kind: Kind::MsI32 });
            }
            if i % 8 == 0 {
                let d = |buf: &[u8]| f64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
                if ticks(d(a), d(b), dt) {
                    out.push(Cand { addr: base + i as u64, kind: Kind::F64 });
                }
            }
        }
    }
    out
}

/// The nearest UObject at or before `addr` (vtable in the image, class name resolves).
fn owner(p: &LinuxProcess, names: &Names, addr: u64) -> Option<(u64, String, String)> {
    for back in (0..0x3000u64).step_by(8) {
        let o = (addr & !7) - back;
        let vt = p.u64(o)?;
        if !(IMAGE_BASE..IMAGE_BASE + 0x700_0000).contains(&vt) {
            continue;
        }
        if let Some(cls) = class_name(p, names, o) {
            let name = p.i32(o + UOBJECT_NAME).and_then(|i| names.get(p, i)).unwrap_or_default();
            if !cls.is_empty() && cls.chars().all(|c| c.is_ascii_graphic()) {
                return Some((o, cls, name));
            }
        }
    }
    None
}

fn main() {
    let pid = LinuxProcess::find(EXE).expect("Ride 4 is not running");
    let p = LinuxProcess::open(pid).expect("open process");
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("find") {
        // every stored time in [LO, HI] seconds, as f32 / f64 seconds or i32 milliseconds
        let (lo, hi): (f64, f64) = (args[1].parse().unwrap(), args[2].parse().unwrap());
        let names = Names::find(&p).expect("FNamePool not found");
        let mut n = 0;
        for (base, buf) in snapshot(&p, &regions(pid)) {
            for i in (0..buf.len().saturating_sub(7)).step_by(4) {
                let f = f32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as f64;
                let ms = i32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as f64 / 1000.0;
                let d = f64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
                for (kind, v) in [("f32", f), ("ms", ms), ("f64", if i % 8 == 0 { d } else { f64::NAN })] {
                    if (lo..=hi).contains(&v) {
                        let addr = base + i as u64;
                        let own = owner(&p, &names, addr).map(|(o, c, nm)| format!("{c} {nm} +{:#x}", addr - o)).unwrap_or_default();
                        println!("{kind}:{addr:#x}  {v:.4}  {own}");
                        n += 1;
                    }
                }
            }
        }
        println!("{n} matches");
        return;
    }

    if args.first().map(String::as_str) == Some("objfind") {
        // like `find`, but only inside live UObjects (from GUObjectArray) whose class or name
        // matches a filter, so the owner is certain
        let (lo, hi): (f64, f64) = (args[1].parse().unwrap(), args[2].parse().unwrap());
        let filter: Vec<String> = args[3..].iter().map(|s| s.to_lowercase()).collect();
        let names = Names::find(&p).expect("FNamePool not found");
        let mut s = ride4_mem::Session::attach(pid).expect("attach");
        s.poll();
        let chain = s.chain().expect("no bike: get on track first");
        let objects = object_array(&p, chain.controller).expect("GUObjectArray not found");
        let mut hits = 0;
        for obj in objects {
            let Some(cls) = class_name(&p, &names, obj) else { continue };
            let name = p.i32(obj + UOBJECT_NAME).and_then(|i| names.get(&p, i)).unwrap_or_default();
            let hay = format!("{cls} {name}").to_lowercase();
            if name.starts_with("Default__") || !filter.iter().any(|f| hay.contains(f)) {
                continue;
            }
            let mut buf = vec![0u8; 0x1000];
            if !p.read(obj, &mut buf) {
                buf.truncate(0x400);
                if !p.read(obj, &mut buf) { continue; }
            }
            for i in (0..buf.len() - 8).step_by(4) {
                let f = f32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as f64;
                let ms = i32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as f64 / 1000.0;
                for (kind, v) in [("f32", f), ("ms", ms)] {
                    if (lo..=hi).contains(&v) {
                        println!("{kind}:{:#x}  {v:.4}  {cls} {name} +{i:#x}", obj + i as u64);
                        hits += 1;
                    }
                }
            }
        }
        println!("{hits} matches");
        return;
    }

    if args.first().map(String::as_str) == Some("objclock") {
        // running clocks inside live UObjects matching a filter (class or name substring)
        let filter: Vec<String> = args[1..].iter().map(|s| s.to_lowercase()).collect();
        let names = Names::find(&p).expect("FNamePool not found");
        let mut s = ride4_mem::Session::attach(pid).expect("attach");
        s.poll();
        let chain = s.chain().expect("no bike: get on track first");
        let objs: Vec<(u64, String)> = object_array(&p, chain.controller).expect("GUObjectArray not found").into_iter()
            .filter_map(|o| {
                let cls = class_name(&p, &names, o)?;
                let name = p.i32(o + UOBJECT_NAME).and_then(|i| names.get(&p, i)).unwrap_or_default();
                let hay = format!("{cls} {name}").to_lowercase();
                (!name.starts_with("Default__") && filter.iter().any(|f| hay.contains(f))).then(|| (o, format!("{cls} {name}")))
            }).collect();
        let read = |o: u64| { let mut b = vec![0u8; 0x1000]; p.read(o, &mut b).then_some(b) };
        let snap = |objs: &[(u64, String)]| objs.iter().map(|(o, _)| read(*o)).collect::<Vec<_>>();
        let a = snap(&objs);
        let t = Instant::now();
        std::thread::sleep(Duration::from_millis(1500));
        let b = snap(&objs);
        let dt = t.elapsed().as_secs_f64();
        for ((o, label), (a, b)) in objs.iter().zip(a.iter().zip(&b)) {
            let (Some(a), Some(b)) = (a, b) else { continue };
            for i in (0..0x1000 - 4).step_by(4) {
                let f = |x: &[u8]| f32::from_le_bytes(x[i..i + 4].try_into().unwrap()) as f64;
                let ms = |x: &[u8]| i32::from_le_bytes(x[i..i + 4].try_into().unwrap()) as f64 / 1000.0;
                for (kind, va, vb) in [("f32", f(a), f(b)), ("ms", ms(a), ms(b))] {
                    if ticks(va, vb, dt) {
                        println!("{kind}:{:#x}  {vb:.3}  {label} +{i:#x}", o + i as u64);
                    }
                }
            }
        }
        return;
    }

    if args.first().map(String::as_str) == Some("refs") {
        // for each object: its outer chain, and every live UObject holding a pointer to it
        let names = Names::find(&p).expect("FNamePool not found");
        let mut s = ride4_mem::Session::attach(pid).expect("attach");
        s.poll();
        let chain = s.chain().expect("no bike: get on track first");
        let objects = object_array(&p, chain.controller).expect("GUObjectArray not found");
        let label = |o: u64| {
            let cls = class_name(&p, &names, o).unwrap_or_default();
            let name = p.i32(o + UOBJECT_NAME).and_then(|i| names.get(&p, i)).unwrap_or_default();
            format!("{cls} {name}")
        };
        println!("controller {:#x} = {}, bike {:#x} = {}", chain.controller, label(chain.controller), chain.bike, label(chain.bike));
        for a in &args[1..] {
            let target = u64::from_str_radix(a.trim_start_matches("0x"), 16).unwrap();
            let mut outer = target;
            let mut path = vec![];
            while outer != 0 && path.len() < 8 {
                path.push(label(outer));
                outer = p.u64(outer + 0x20).unwrap_or(0);
            }
            println!("\n{target:#x}: {}", path.join("  <-  "));
            for &o in &objects {
                let mut buf = vec![0u8; 0x1200];
                if !p.read(o, &mut buf) { continue; }
                for (i, c) in buf.chunks_exact(8).enumerate() {
                    if u64::from_le_bytes(c.try_into().unwrap()) == target && o != target {
                        println!("   referenced by {:#x} +{:#x}  {}", o, 8 * i, label(o));
                    }
                }
            }
        }
        return;
    }

    if args.first().map(String::as_str) == Some("watch") {
        let secs: f64 = args[1].parse().unwrap();
        let cands: Vec<Cand> = args[2..].iter().map(|a| {
            let (k, h) = a.split_once(':').unwrap_or(("f32", a));
            let kind = match k { "f64" => Kind::F64, "ms" => Kind::MsI32, _ => Kind::F32 };
            Cand { addr: u64::from_str_radix(h.trim_start_matches("0x"), 16).unwrap(), kind }
        }).collect();
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f64() < secs {
            let vals: Vec<String> = cands.iter().map(|&c| value(&p, c).map_or("-".into(), |v| format!("{v:.3}"))).collect();
            println!("{:7.2} {}", t0.elapsed().as_secs_f64(), vals.join(" "));
            std::thread::sleep(Duration::from_millis(50));
        }
        return;
    }

    let names = Names::find(&p).expect("FNamePool not found");
    let regs = regions(pid);
    let t0 = Instant::now();
    let s0 = snapshot(&p, &regs);
    let read0 = t0.elapsed().as_secs_f64();
    std::thread::sleep(Duration::from_secs(2));
    let t1 = Instant::now();
    let s1 = snapshot(&p, &regs);
    // each snapshot takes a while to read; compare at the midpoints of the reads
    let dt = (t1 - t0).as_secs_f64() + (t1.elapsed().as_secs_f64() - read0) / 2.0;
    let mut cands = first_pass(&s0, &s1, dt);
    drop((s0, s1));
    println!("pass 1 (dt {dt:.2}s): {} candidates", cands.len());

    for round in 2..=6 {
        let before: Vec<Option<f64>> = cands.iter().map(|&c| value(&p, c)).collect();
        let t = Instant::now();
        std::thread::sleep(Duration::from_millis(1500));
        let dt = t.elapsed().as_secs_f64();
        cands = cands.iter().zip(before).filter(|(&c, b)| matches!((b, value(&p, c)), (Some(a), Some(b)) if ticks(*a, b, dt)))
            .map(|(&c, _)| c).collect();
        println!("pass {round}: {} candidates", cands.len());
    }
    for c in &cands {
        let v = value(&p, *c).unwrap_or(f64::NAN);
        let own = owner(&p, &names, c.addr).map(|(o, cls, n)| format!("{cls} {n} +{:#x}", c.addr - o)).unwrap_or_default();
        println!("{:?}:{:#x}  {:9.3}  {own}", c.kind, c.addr, v);
    }
}

/// Every live UObject, from GUObjectArray. Located through one known object: its InternalIndex
/// (+0xC) must appear at the matching slot of a chunk table that some `.data` pointer leads to.
fn object_array(p: &LinuxProcess, known: u64) -> Option<Vec<u64>> {
    use ride4_mem::offsets::DATA;
    let idx = p.i32(known + 0xc)? as u64;
    let (chunk, slot) = (idx / 65536, idx % 65536);
    let mut field = None;
    for f in (DATA.0..DATA.1).step_by(8) {
        let Some(table) = p.u64(f).filter(|&t| ride4_mem::process::is_ptr(t)) else { continue };
        let Some(c) = p.u64(table + 8 * chunk).filter(|&c| ride4_mem::process::is_ptr(c)) else { continue };
        if p.u64(c + 24 * slot) == Some(known) {
            field = Some(f);
            break;
        }
    }
    let field = field?;
    let table = p.u64(field)?;
    let num = p.i32(field + 0x14)? as u64;
    let mut out = Vec::with_capacity(num as usize);
    for ci in 0..num.div_ceil(65536) {
        let c = p.u64(table + 8 * ci)?;
        let n = (num - ci * 65536).min(65536) as usize;
        let mut buf = vec![0u8; n * 24];
        if !p.read(c, &mut buf) { continue; }
        out.extend(buf.chunks_exact(24).map(|e| u64::from_le_bytes(e[..8].try_into().unwrap())).filter(|&o| o != 0));
    }
    Some(out)
}
