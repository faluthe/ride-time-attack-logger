//! Time the per-sample cost of `Session::poll`.
use ride4_mem::{LinuxProcess, Session, EXE};
use std::time::Instant;

fn main() {
    let pid = LinuxProcess::find(EXE).expect("Ride 4 is not running");
    let mut s = Session::attach(pid).expect("attach");
    s.poll();
    let n = 500;
    let t = Instant::now();
    let mut some = 0;
    for _ in 0..n {
        some += s.poll().is_some() as u32;
    }
    let per = t.elapsed() / n;
    let t = Instant::now();
    for _ in 0..n {
        let _ = s.chain();
    }
    println!("poll: {per:?} each ({some}/{n} with a bike); chain: {:?} each; alive: {:?}", t.elapsed() / n, {
        let t = Instant::now();
        for _ in 0..n { let _ = s.process.alive(); }
        t.elapsed() / n
    });
}
