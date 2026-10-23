//! Unreal pak files (version 9, UE 4.25): unencrypted index, zlib or stored entries.

use crate::reader::{err, Reader, Result};
use flate2::read::ZlibDecoder;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const MAGIC: u32 = 0x5A6F_12E1;
/// Size of the per-entry header that precedes stored (uncompressed) data.
const ENTRY_HEADER: u64 = 8 * 3 + 4 + 20 + 5;

#[derive(Clone, Debug)]
struct Entry {
    offset: u64,
    size: u64,
    uncompressed: u64,
    method: u32,
    blocks: Vec<(u64, u64)>,
    encrypted: bool,
}

pub struct Pak {
    pub path: PathBuf,
    pub mount: String,
    methods: Vec<String>,
    entries: BTreeMap<String, Entry>,
}

impl Pak {
    pub fn open(path: &Path) -> Result<Self> {
        let mut f = File::open(path)?;
        let size = f.seek(SeekFrom::End(0))?;
        let mut tail = vec![0u8; 400.min(size as usize)];
        f.seek(SeekFrom::Start(size - tail.len() as u64))?;
        f.read_exact(&mut tail)?;
        let Some(i) = tail.windows(4).rposition(|w| w == MAGIC.to_le_bytes()) else {
            return err(format!("{}: no pak footer", path.display()));
        };
        let mut r = Reader::new(&tail[i..]);
        let (_magic, _version, index_offset, index_size) = (r.u32()?, r.u32()?, r.i64()?, r.i64()?);
        if i > 0 && tail[i - 1] != 0 {
            return err(format!("{}: encrypted index", path.display()));
        }
        // after the 20-byte index hash and a frozen-index flag: compression method names, 32 bytes each
        let names = &tail[(i + 24 + 20 + 1).min(tail.len())..];
        let mut methods = vec!["None".to_string()];
        methods.extend(names.chunks_exact(32).map(|c| c.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()));

        let mut index = vec![0u8; index_size as usize];
        f.seek(SeekFrom::Start(index_offset as u64))?;
        f.read_exact(&mut index)?;
        let mut r = Reader::new(&index);
        let mount = r.fstring()?;
        let count = r.i32()?;
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            let name = r.fstring()?;
            let (offset, size, uncompressed, method) = (r.i64()? as u64, r.i64()? as u64, r.i64()? as u64, r.u32()?);
            r.take(20)?; // sha1
            let mut blocks = vec![];
            if method != 0 {
                for _ in 0..r.i32()? {
                    blocks.push((r.i64()? as u64, r.i64()? as u64));
                }
            }
            let flags = r.u8()?;
            r.u32()?; // compression block size
            entries.insert(name, Entry { offset, size, uncompressed, method, blocks, encrypted: flags & 1 != 0 });
        }
        Ok(Self { path: path.to_owned(), mount, methods, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let Some(e) = self.entries.get(name) else { return err(format!("{name} not in pak")) };
        if e.encrypted {
            return err(format!("{name} is encrypted"));
        }
        let mut f = File::open(&self.path)?;
        if e.method == 0 {
            let mut out = vec![0u8; e.size as usize];
            f.seek(SeekFrom::Start(e.offset + ENTRY_HEADER))?;
            f.read_exact(&mut out)?;
            return Ok(out);
        }
        let method = self.methods.get(e.method as usize).map(String::as_str).unwrap_or("?");
        if !method.eq_ignore_ascii_case("zlib") {
            return err(format!("{name}: unsupported compression {method}"));
        }
        let mut out = Vec::with_capacity(e.uncompressed as usize);
        for &(start, end) in &e.blocks {
            let mut block = vec![0u8; (end - start) as usize];
            f.seek(SeekFrom::Start(e.offset + start))?;
            f.read_exact(&mut block)?;
            ZlibDecoder::new(&block[..]).read_to_end(&mut out)?;
        }
        out.truncate(e.uncompressed as usize);
        Ok(out)
    }
}

/// Every pak in the game's `Content/Paks`, merged; later (patch) paks override earlier ones.
pub struct Game {
    paks: Vec<Pak>,
    index: BTreeMap<String, usize>,
}

impl Game {
    pub fn open(game_root: &Path) -> Result<Self> {
        let dir = game_root.join("ride4/Content/Paks");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| crate::reader::Error(format!("{}: {e}", dir.display())))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pak"))
            .collect();
        files.sort();
        let mut paks = vec![];
        let mut index = BTreeMap::new();
        for f in files {
            let pak = Pak::open(&f)?;
            for n in pak.names() {
                index.insert(n.clone(), paks.len());
            }
            paks.push(pak);
        }
        Ok(Self { paks, index })
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.index.keys()
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        match self.index.get(name) {
            Some(&i) => self.paks[i].read(name),
            None => err(format!("{name} not found in any pak")),
        }
    }
}
