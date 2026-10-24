//! Session CSV files: one row per frame.
//!
//! Columns: `t,x,y,z,fx,fy,fz,side_z,up_z,throttle,brake,rear,gear,rpm,clock,laps,last_lap_ms`.
//! Older sessions have only the first nine (before inputs were found) or fourteen (before the
//! game's lap timing was found); they still load. In modes without lap timing, `laps` is -1.

use crate::sample::{GameTiming, Inputs, Sample};
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

pub const HEADER: &str = "t,x,y,z,fx,fy,fz,side_z,up_z,throttle,brake,rear,gear,rpm,clock,laps,last_lap_ms";

pub struct SessionWriter {
    out: BufWriter<File>,
    rows: usize,
}

impl SessionWriter {
    pub fn create(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = BufWriter::new(File::create(path)?);
        writeln!(out, "{HEADER}")?;
        Ok(Self { out, rows: 0 })
    }

    pub fn write(&mut self, s: &Sample) -> io::Result<()> {
        let [x, y, z] = s.pos;
        let [fx, fy, fz] = s.fwd;
        write!(self.out, "{:.4},{x:.3},{y:.3},{z:.3},{fx:.5},{fy:.5},{fz:.5},{:.5},{:.5}", s.t, s.side_z, s.up_z)?;
        if let Some(i) = s.inputs {
            write!(self.out, ",{:.3},{:.3},{:.0},{},{:.4}", i.throttle, i.brake, i.rear, i.gear, i.rpm)?;
            match s.timing {
                Some(g) => write!(self.out, ",{:.4},{},{}", g.clock, g.laps, g.last_lap_ms)?,
                None => write!(self.out, ",0,-1,0")?,
            }
        }
        writeln!(self.out)?;
        self.rows += 1;
        if self.rows % 500 == 0 {
            self.out.flush()?;
        }
        Ok(())
    }

    /// Get every row so far onto disk, e.g. before something else reads the file.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn finish(mut self) -> io::Result<usize> {
        self.out.flush()?;
        Ok(self.rows)
    }
}

/// Load a session, skipping malformed rows (e.g. a half-written last line).
pub fn read_session(path: &Path) -> io::Result<Vec<Sample>> {
    let mut out = Vec::new();
    for line in BufReader::new(File::open(path)?).lines().skip(1) {
        let line = line?;
        let v: Vec<f64> = match line.split(',').map(str::parse).collect() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if ![9, 14, 17].contains(&v.len()) {
            continue;
        }
        let f = |i: usize| v[i] as f32;
        out.push(Sample {
            t: v[0],
            pos: [f(1), f(2), f(3)],
            fwd: [f(4), f(5), f(6)],
            side_z: f(7),
            up_z: f(8),
            inputs: (v.len() >= 14).then(|| Inputs { throttle: f(9), brake: f(10), rear: f(11), gear: v[12] as i32, rpm: f(13) }),
            timing: (v.len() == 17 && v[15] >= 0.0).then(|| GameTiming { clock: v[14], laps: v[15] as i32, last_lap_ms: v[16] as i32 }),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_column_sets() {
        let dir = std::env::temp_dir().join(format!("rta-csv-{}", std::process::id()));
        let path = dir.join("s.csv");
        let mut a = Sample::from_quat(0.5, [1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0],
            Some(Inputs { throttle: 0.5, brake: 0.25, rear: 1.0, gear: 3, rpm: 0.75 }));
        a.timing = Some(GameTiming { clock: 12.5, laps: 2, last_lap_ms: 138278 });
        let mut w = SessionWriter::create(&path).unwrap();
        w.write(&a).unwrap();
        w.write(&Sample { timing: None, ..a }).unwrap();
        w.write(&Sample { inputs: None, timing: None, ..a }).unwrap();
        w.finish().unwrap();
        let back = read_session(&path).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].timing.unwrap().last_lap_ms, 138278);
        assert_eq!(back[0].inputs.unwrap().gear, 3);
        assert!(back[1].timing.is_none() && back[1].inputs.is_some());
        assert!(back[2].inputs.is_none());
        assert!((back[0].fwd[0] - 1.0).abs() < 1e-6);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
