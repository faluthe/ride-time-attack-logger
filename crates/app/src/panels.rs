//! The dashboard's panels, switched on and off from the header's "panels" menu and remembered
//! between runs. The names match the site's panel toggles: graphs speed, lean, inputs, gear, rpm
//! and g; widgets tach, rider and timer, plus the logger's own throttle / brake bars.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub const GRAPHS: [&str; 6] = ["speed", "lean", "inputs", "gear", "rpm", "g"];
pub const WIDGETS: [&str; 4] = ["tach", "rider", "bars", "timer"];

/// Settings file: `$XDG_CONFIG_HOME/ride-time-attack/logger.json` (`~/.config/...` by default).
fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
    });
    base.join("ride-time-attack/logger.json")
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Saved {
    /// Only the panels the user switched; the rest are on.
    #[serde(default)]
    panels: BTreeMap<String, bool>,
}

pub struct Panels {
    saved: Saved,
    /// Don't write the settings file (screenshots shouldn't change the user's layout).
    readonly: bool,
}

impl Panels {
    pub fn load(readonly: bool) -> Self {
        let saved = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { saved, readonly }
    }

    pub fn on(&self, name: &str) -> bool {
        self.saved.panels.get(name).copied().unwrap_or(true)
    }

    pub fn set(&mut self, name: &str, on: bool) {
        self.saved.panels.insert(name.to_string(), on);
        if self.readonly {
            return;
        }
        let p = path();
        let write = || -> std::io::Result<()> {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&p, serde_json::to_vec_pretty(&self.saved)?)
        };
        if let Err(e) = write() {
            eprintln!("can't save panel settings to {}: {e}", p.display());
        }
    }

    /// The menu: a checkbox per panel, graphs and widgets apart.
    pub fn menu(&mut self, ui: &imgui::Ui) {
        for (title, names) in [("graphs", &GRAPHS[..]), ("widgets", &WIDGETS[..])] {
            ui.text_disabled(title);
            for &n in names {
                let mut on = self.on(n);
                if ui.checkbox(n, &mut on) {
                    self.set(n, on);
                }
            }
        }
    }
}
