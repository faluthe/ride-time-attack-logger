//! Career event definitions from the game's data tables: which track, conditions, how scored.

use crate::asset::datatable;
use crate::pak::Game;
use crate::reader::Result;
use crate::tracks::track_key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const CAREER: &str = "ride4/Content/assets/data/Career/";

/// (data table, type key, label, what the result measures)
const TABLES: &[(&str, &str, &str, &str)] = &[
    ("TimeAttackEventsConfiguration_DT", "time_attack", "Time attack", "time"),
    ("GateEventsConfiguration_DT", "track_knowledge", "Track knowledge", "time"),
    ("TrackDayEventsConfigutations_DT", "track_day", "Track day", "overtakes"),
    ("CareerRaceEventConfiguration_DT", "race", "Race", "position"),
    ("CareerChampionshipEventConfiguration_DT", "championship", "Championship", "position"),
    ("CareerEnduranceEventConfiguration_DT", "endurance", "Endurance", "position"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub kind: String,
    pub type_label: String,
    /// "time", "overtakes" or "position"
    pub metric: String,
    pub track: Option<String>,
    pub full_lap: Option<bool>,
    pub weather: Option<String>,
    pub hour: Option<i64>,
}

pub type Events = BTreeMap<String, Event>;

/// `EWeather::Rain` -> `Rain`
pub fn enum_tail(s: &str) -> &str {
    s.rsplit("::").next().unwrap_or(s)
}

pub fn extract(game: &Game) -> Result<Events> {
    let mut out = BTreeMap::new();
    for &(table, kind, label, metric) in TABLES {
        let base = format!("{CAREER}{table}");
        let rows = datatable(&game.read(&format!("{base}.uasset"))?, &game.read(&format!("{base}.uexp"))?)?;
        for (id, row) in rows {
            if !id.starts_with("EVENT-") {
                continue;
            }
            let cfg = &row["CompetitionConfiguration"];
            let wt = &cfg["WeatherAndTimeOptions"];
            out.insert(id, Event {
                kind: kind.into(),
                type_label: label.into(),
                metric: metric.into(),
                track: cfg["TrackVariant"].as_str().filter(|s| !s.is_empty()).map(track_key),
                full_lap: row["IsFullLap"].as_bool(),
                weather: wt["WeatherConditions"].as_str().map(enum_tail).filter(|s| !s.is_empty()).map(Into::into),
                hour: wt["RaceStartTime"]["CustomTimeValue"].as_i64(),
            });
        }
    }
    Ok(out)
}
