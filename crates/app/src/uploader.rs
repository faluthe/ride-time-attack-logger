//! Background uploader: after each completed lap, export the sessions and sync them to the site's
//! R2 bucket, off the UI and reader threads.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

#[derive(Clone, Debug)]
pub enum Status {
    /// No credentials, or turned off; says why.
    Off(String),
    /// Waiting for a lap.
    Idle,
    Working,
    Done { at: Instant, uploaded: usize, bytes: usize },
    Failed { at: Instant, error: String },
}

pub struct Uploader {
    pub status: Arc<Mutex<Status>>,
    /// Where exports are written before syncing.
    pub out: PathBuf,
    /// Send `()` when a lap is done; `None` when off.
    pub tx: Option<Sender<()>>,
}

impl Uploader {
    pub fn spawn(sessions: PathBuf, enabled: bool) -> Self {
        let out = export::data_dir().join("export");
        let store = match enabled.then(upload::R2::from_env) {
            None => Err("off (--no-upload)".to_string()),
            Some(Err(e)) => Err(format!("off: {e}")),
            Some(Ok(s)) => Ok(s),
        };
        let store = match store {
            Ok(s) => s,
            Err(why) => return Self { status: Arc::new(Mutex::new(Status::Off(why))), out, tx: None },
        };
        let status = Arc::new(Mutex::new(Status::Idle));
        let (tx, rx) = channel();
        let mut opts = export::Options::new(out.clone());
        opts.sessions = sessions;
        let st = status.clone();
        thread::Builder::new().name("uploader".into()).spawn(move || run(store, opts, rx, st)).expect("spawn uploader");
        Self { status, out, tx: Some(tx) }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

fn run(store: upload::R2, opts: export::Options, rx: Receiver<()>, status: Arc<Mutex<Status>>) {
    while rx.recv().is_ok() {
        // laps that finished while the last upload ran are covered by this one
        while rx.try_recv().is_ok() {}
        *status.lock().unwrap() = Status::Working;
        let result = export::export(&opts).and_then(|()| upload::sync(&store, &opts.out));
        let at = Instant::now();
        *status.lock().unwrap() = match result {
            Ok(s) => {
                println!("uploaded {} files ({} KB), {} unchanged, {} deleted", s.uploaded.len(), s.bytes / 1024, s.unchanged, s.deleted.len());
                Status::Done { at, uploaded: s.uploaded.len(), bytes: s.bytes }
            }
            Err(e) => {
                eprintln!("upload failed: {e}");
                Status::Failed { at, error: e.to_string() }
            }
        };
    }
}
