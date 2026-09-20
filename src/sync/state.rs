use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

const FILE: &str = "sync-state.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub last_sync: Option<String>,
}

pub fn read_sync_state(store_dir: &Path) -> SyncState {
    fs::read_to_string(store_dir.join(FILE))
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

pub fn write_sync_state(store_dir: &Path, last_sync: &str) -> anyhow::Result<()> {
    fs::create_dir_all(store_dir)?;
    let state = SyncState {
        last_sync: Some(last_sync.to_string()),
    };
    fs::write(
        store_dir.join(FILE),
        format!("{}\n", serde_json::to_string_pretty(&state)?),
    )?;
    Ok(())
}

pub fn parse_duration_ms(value: &str) -> Option<i64> {
    let (amount, unit) = value.split_at(value.len().checked_sub(1)?);
    let amount: i64 = amount.parse().ok()?;
    let multiplier = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    };
    Some(amount * multiplier)
}

pub fn is_sync_stale(store_dir: &Path, max_age: &str) -> bool {
    let Some(max_ms) = parse_duration_ms(max_age) else {
        eprintln!(
            "Invalid max_age format: \"{max_age}\". Expected format: \"30m\", \"1h\", \"1d\""
        );
        return false;
    };
    let Some(last) = read_sync_state(store_dir).last_sync else {
        return true;
    };
    let Ok(last) =
        time::OffsetDateTime::parse(&last, &time::format_description::well_known::Rfc3339)
    else {
        return true;
    };
    (time::OffsetDateTime::now_utc() - last).whole_milliseconds() > max_ms as i128
}
