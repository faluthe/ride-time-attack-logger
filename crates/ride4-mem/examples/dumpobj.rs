//! Print an object's fields as f32 / i32 / pointer, to map unknown structs.
//! `cargo run --release -p ride4-mem --example dumpobj -- ADDR [LEN]`
use ride4_mem::{LinuxProcess, Memory, EXE};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let addr = u64::from_str_radix(a[0].trim_start_matches("0x"), 16).unwrap();
    let len = a.get(1).map_or(0x200, |s| usize::from_str_radix(s.trim_start_matches("0x"), 16).unwrap());
    let p = LinuxProcess::open(LinuxProcess::find(EXE).expect("not running")).unwrap();
    let mut b = vec![0u8; len];
    assert!(p.read(addr, &mut b), "unreadable");
    for i in (0..len).step_by(4) {
        let w = &b[i..i + 4];
        let (f, n) = (f32::from_le_bytes(w.try_into().unwrap()), i32::from_le_bytes(w.try_into().unwrap()));
        let q = (i % 8 == 0 && i + 8 <= len).then(|| u64::from_le_bytes(b[i..i + 8].try_into().unwrap()));
        let ptr = q.filter(|&q| ride4_mem::process::is_ptr(q)).map(|q| format!("ptr {q:#x}")).unwrap_or_default();
        if n != 0 {
            println!("+{i:#05x}  i32 {n:<12} f32 {f:<14.4} {ptr}");
        }
    }
}
