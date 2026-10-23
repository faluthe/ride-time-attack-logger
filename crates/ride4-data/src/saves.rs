//! The player's offline leaderboards and career progress from the save games.

use crate::events::{enum_tail, Event, Events};
use crate::gvas;
use crate::reader::{err, Result};
use crate::tracks::track_key;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct BoardLap {
    pub time: f64,
    /// e.g. `Suzuki_GSX-R-1000R-2019`
    pub bike: String,
    pub pp: i64,
    pub front: Option<String>,
    pub rear: Option<String>,
    pub physics: String,
    pub wet: bool,
}

/// Best laps for one track and bike class, fastest first.
#[derive(Clone, Debug, Serialize)]
pub struct Leaderboard {
    pub track: String,
    pub class: String,
    pub laps: Vec<BoardLap>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CareerResult {
    pub id: String,
    #[serde(flatten)]
    pub event: Option<Event>,
    pub state: String,
    /// lap time, overtakes or finishing position, depending on the event's metric
    pub result: f64,
    pub goal: f64,
    /// cumulative split times, seconds
    pub checkpoints: Vec<f64>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn tyre(v: &Value) -> Option<String> {
    let t = enum_tail(v.as_str()?);
    (t != "nocompound").then(|| t.to_string())
}

fn load(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path).map_err(|e| crate::Error(format!("{}: {e}", path.display())))?;
    gvas::load(&bytes)
}

pub fn leaderboards(saves: &Path) -> Result<Vec<Leaderboard>> {
    let lb = load(&saves.join("OfflineLeaderboards.sav"))?;
    let Some(boards) = lb["OfflineLeaderboards"].as_array() else { return err("no OfflineLeaderboards") };
    Ok(boards.iter().map(|b| {
        let id = b["LeaderboardId"].as_str().unwrap_or_default();
        let (trk, cls) = id.split_once('-').unwrap_or((id, ""));
        let mut laps: Vec<BoardLap> = b["Rows"].as_array().into_iter().flatten().map(|r| BoardLap {
            time: round3(r["Time"].as_f64().unwrap_or(0.0)),
            bike: r["VehicleKey"].as_str().unwrap_or_default().into(),
            pp: r["PerformancePoints"].as_i64().unwrap_or(0),
            front: tyre(&r["FrontTyreCompound"]),
            rear: tyre(&r["RearTyreCompound"]),
            physics: enum_tail(r["BikePhysics"].as_str().unwrap_or_default()).into(),
            wet: r["IsWetRoad"].as_bool().unwrap_or(false),
        }).collect();
        laps.sort_by(|a, b| a.time.total_cmp(&b.time));
        Leaderboard { track: track_key(trk), class: cls.into(), laps }
    }).collect())
}

pub fn career(saves: &Path, events: &Events) -> Result<Vec<CareerResult>> {
    let prof = load(&saves.join("PlayerProfile.sav"))?;
    let Some(progress) = prof["CareerEventsProgressData"].as_array() else { return err("no CareerEventsProgressData") };
    Ok(progress.iter().filter_map(|kv| {
        let (id, p) = (kv[0].as_str()?, &kv[1]);
        let state = p["ProgressState"].as_str().unwrap_or_default();
        if state.ends_with("NeverPlayed") {
            return None;
        }
        Some(CareerResult {
            id: id.into(),
            event: events.get(id).cloned(),
            state: enum_tail(state).into(),
            result: round3(p["BestResult"].as_f64().unwrap_or(0.0)),
            goal: round3(p["BestClearedObjectiveGoal"].as_f64().unwrap_or(0.0)),
            checkpoints: p["BestLapChekpoints"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(round3).collect(),
        })
    }).collect())
}
