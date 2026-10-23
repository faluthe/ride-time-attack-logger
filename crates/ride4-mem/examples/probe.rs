//! Attach to the running game and print the chain plus live channels at 10 Hz.
//! `cargo run -p ride4-mem --example probe`

use ride4_mem::{LinuxProcess, Session, EXE};
use std::{thread, time::Duration};

fn main() {
    let pid = LinuxProcess::find(EXE).expect("Ride 4 is not running");
    let mut s = Session::attach(pid).expect("attach");
    println!("attached to pid {pid}");
    loop {
        match s.poll() {
            Some(b) => {
                if let Some(c) = s.chain() {
                    print!("[{} {:#x}] ", s.class_of(c.controller).unwrap_or_default(), c.bike);
                }
                print!(
                    "pos {:8.2} {:8.2} {:6.2}  gear {}  rpm {:.2}  thr {:.2}  brk {:.2}  rear {:.0}",
                    b.pos[0], b.pos[1], b.pos[2], b.gear, b.rpm, b.throttle, b.brake, b.rear
                );
                match s.timing() {
                    Some(t) => println!("  clock {:.3}  laps {}  last {:.3}  best {:.3}",
                        t.race_clock, t.laps, t.last_lap_ms as f64 / 1000.0, t.best_lap),
                    None => println!("  (no lap timing in this mode)"),
                }
            }
            None => println!("no bike (menu or loading)"),
        }
        thread::sleep(Duration::from_millis(100));
    }
}
