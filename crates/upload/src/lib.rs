//! Upload `rta-export` output to the bucket the site reads `/data/*` from.
//!
//! [`sync`] makes the bucket match an export directory, sending only what's new or changed:
//! lap files (`laps/<id>.bin.gz`) and the other JSON first, `laps.json` last so the live index
//! never points at a missing file, then lap files the index no longer lists are deleted.
//! Changes are found by comparing each file's MD5 with the object's ETag.
//!
//! [`Store`] is the bucket; [`R2`] talks to Cloudflare R2 over its S3 API with a token from the
//! environment or a `.env` file. Single user for now: the token can write the whole bucket.

mod r2;

pub use r2::R2;

use md5::{Digest, Md5};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// An object in the bucket.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub key: String,
    /// MD5 of the content, lowercase hex, no quotes
    pub etag: String,
}

pub trait Store {
    /// Every object whose key starts with `prefix`.
    fn list(&self, prefix: &str) -> Res<Vec<Object>>;
    fn put(&self, key: &str, body: &[u8], content_type: &str) -> Res<()>;
    fn delete(&self, key: &str) -> Res<()>;
}

#[derive(Debug, Default)]
pub struct Summary {
    /// keys uploaded, in order
    pub uploaded: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: usize,
    pub bytes: usize,
}

const INDEX: &str = "laps.json";

fn md5_hex(b: &[u8]) -> String {
    hex::encode(Md5::digest(b))
}

/// Local files to upload as (key, path), lap files first and the lap index last.
fn local_files(dir: &Path) -> Res<Vec<(String, std::path::PathBuf)>> {
    if !dir.join(INDEX).is_file() {
        return Err(format!("no {INDEX} in {}; run rta-export first", dir.display()).into());
    }
    let mut laps = vec![];
    if let Ok(rd) = std::fs::read_dir(dir.join("laps")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".bin.gz") {
                laps.push((format!("laps/{name}"), e.path()));
            }
        }
    }
    let mut json = vec![];
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") && name != INDEX {
            json.push((name, e.path()));
        }
    }
    laps.sort();
    json.sort();
    Ok(laps.into_iter().chain(json).chain([(INDEX.to_string(), dir.join(INDEX))]).collect())
}

/// Make the bucket match the export in `dir`. See the crate docs.
pub fn sync(store: &dyn Store, dir: &Path) -> Res<Summary> {
    let files = local_files(dir)?;
    let remote: HashMap<String, String> = store.list("")?.into_iter().map(|o| (o.key, o.etag)).collect();
    let local_laps: HashSet<&str> = files.iter().map(|(k, _)| k.as_str()).filter(|k| k.starts_with("laps/")).collect();
    let stale: Vec<&String> = remote.keys().filter(|k| k.starts_with("laps/") && !local_laps.contains(k.as_str())).collect();
    // an export from a machine without the sessions would otherwise wipe the site's laps
    if local_laps.is_empty() && !stale.is_empty() {
        return Err(format!("{} has no lap files but the bucket has {}; not replacing them", dir.display(), stale.len()).into());
    }

    let mut s = Summary::default();
    for (key, path) in &files {
        let body = std::fs::read(path)?;
        if remote.get(key).is_some_and(|etag| *etag == md5_hex(&body)) {
            s.unchanged += 1;
            continue;
        }
        let ty = if key.ends_with(".json") { "application/json" } else { "application/octet-stream" };
        store.put(key, &body, ty).map_err(|e| format!("{key}: {e}"))?;
        s.bytes += body.len();
        s.uploaded.push(key.clone());
    }
    let mut stale: Vec<String> = stale.into_iter().cloned().collect();
    stale.sort();
    for key in stale {
        store.delete(&key).map_err(|e| format!("{key}: {e}"))?;
        s.deleted.push(key);
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Mem(RefCell<BTreeMap<String, Vec<u8>>>);

    impl Store for Mem {
        fn list(&self, prefix: &str) -> Res<Vec<Object>> {
            let m = self.0.borrow();
            Ok(m.iter().filter(|(k, _)| k.starts_with(prefix)).map(|(k, v)| Object { key: k.clone(), etag: md5_hex(v) }).collect())
        }
        fn put(&self, key: &str, body: &[u8], _: &str) -> Res<()> {
            self.0.borrow_mut().insert(key.into(), body.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Res<()> {
            self.0.borrow_mut().remove(key);
            Ok(())
        }
    }

    fn export(files: &[(&str, &str)]) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rta-upload-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("laps")).unwrap();
        for (k, v) in files {
            std::fs::write(dir.join(k), v).unwrap();
        }
        dir
    }

    #[test]
    fn uploads_new_and_changed_index_last_then_prunes() {
        let store = Mem::default();
        store.put("laps/a.bin.gz", b"a", "").unwrap();
        store.put("laps/gone.bin.gz", b"old", "").unwrap();
        store.put("tracks.json", b"{}", "").unwrap();
        let dir = export(&[
            ("laps/a.bin.gz", "a"),
            ("laps/b.bin.gz", "b"),
            ("tracks.json", "{}"),
            ("career.json", "[1]"),
            ("laps.json", "{\"laps\":[]}"),
        ]);
        let s = sync(&store, &dir).unwrap();
        assert_eq!(s.uploaded, ["laps/b.bin.gz", "career.json", "laps.json"]);
        assert_eq!(s.deleted, ["laps/gone.bin.gz"]);
        assert_eq!(s.unchanged, 2);
        let keys: Vec<String> = store.0.borrow().keys().cloned().collect();
        assert_eq!(keys, ["career.json", "laps.json", "laps/a.bin.gz", "laps/b.bin.gz", "tracks.json"]);

        let again = sync(&store, &dir).unwrap();
        assert!(again.uploaded.is_empty() && again.deleted.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_to_wipe_laps_with_an_empty_export() {
        let store = Mem::default();
        store.put("laps/a.bin.gz", b"a", "").unwrap();
        let dir = export(&[("laps.json", "{\"laps\":[]}")]);
        assert!(sync(&store, &dir).is_err());
        assert!(store.0.borrow().contains_key("laps/a.bin.gz"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn needs_an_index() {
        let dir = export(&[("laps/a.bin.gz", "a")]);
        assert!(sync(&Mem::default(), &dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
