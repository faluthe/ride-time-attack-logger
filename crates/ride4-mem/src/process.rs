//! Raw access to another process's memory.

use std::fs::{self, File};
use std::os::unix::fs::FileExt;

/// Anything we can read target memory from. Kept tiny so other OS backends can slot in.
pub trait Memory {
    /// Fill `buf` from `addr`. Returns false if any part of the range is unreadable.
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool;

    fn u64(&self, addr: u64) -> Option<u64> {
        let mut b = [0; 8];
        self.read(addr, &mut b).then(|| u64::from_le_bytes(b))
    }
    fn i32(&self, addr: u64) -> Option<i32> {
        let mut b = [0; 4];
        self.read(addr, &mut b).then(|| i32::from_le_bytes(b))
    }
    fn f32(&self, addr: u64) -> Option<f32> {
        let mut b = [0; 4];
        self.read(addr, &mut b).then(|| f32::from_le_bytes(b))
    }
    fn u16(&self, addr: u64) -> Option<u16> {
        let mut b = [0; 2];
        self.read(addr, &mut b).then(|| u16::from_le_bytes(b))
    }
}

/// Plausible user-space pointer on x86-64 (filters nulls and garbage before we dereference).
pub fn is_ptr(p: u64) -> bool {
    (0x10000..0x7fff_ffff_ffff).contains(&p)
}

/// A Linux process read through `/proc/<pid>/mem` (works for Proton/Wine games).
pub struct LinuxProcess {
    pub pid: u32,
    mem: File,
}

impl LinuxProcess {
    pub fn open(pid: u32) -> std::io::Result<Self> {
        Ok(Self { pid, mem: File::open(format!("/proc/{pid}/mem"))? })
    }

    /// First process whose command line mentions `exe`.
    pub fn find(exe: &str) -> Option<u32> {
        fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let cmd = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            String::from_utf8_lossy(&cmd).contains(exe).then_some(pid)
        })
    }

    pub fn alive(&self) -> bool {
        fs::metadata(format!("/proc/{}", self.pid)).is_ok()
    }
}

impl Memory for LinuxProcess {
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        is_ptr(addr) && matches!(self.mem.read_at(buf, addr), Ok(n) if n == buf.len())
    }
}
