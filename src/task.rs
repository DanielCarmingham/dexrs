use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub completed: bool,
    pub result: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    #[serde(rename = "blockedBy")]
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub children: Vec<String>,
}

impl Task {
    pub fn new(
        id: String,
        name: String,
        description: Option<String>,
        priority: Option<String>,
    ) -> Self {
        let now = timestamp();
        Self {
            id,
            parent_id: None,
            name,
            description,
            priority: priority.map(Priority::String),
            completed: false,
            result: None,
            metadata: None,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            started_at: None,
            completed_at: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Priority {
    String(String),
    Number(serde_json::Number),
}

impl fmt::Display for Priority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Priority::String(priority) => formatter.write_str(priority),
            Priority::Number(priority) => write!(formatter, "{priority}"),
        }
    }
}

pub fn parse_tasks_jsonl(input: &str) -> anyhow::Result<Vec<Task>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

pub fn serialize_tasks_jsonl(tasks: &[Task]) -> anyhow::Result<String> {
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    let mut output = String::new();
    for task in ordered {
        output.push_str(&serde_json::to_string(&task)?);
        output.push('\n');
    }

    Ok(output)
}

pub fn timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC3339 formatting should not fail")
}

pub fn generate_id(existing_ids: impl Fn(&str) -> bool) -> String {
    for attempt in 0..u64::MAX {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let value = nanos ^ ((std::process::id() as u128) << 32) ^ attempt as u128;
        let id = base36_8(value);
        if !existing_ids(&id) {
            return id;
        }
    }

    unreachable!("id generation exhausted u64 attempts")
}

fn base36_8(mut value: u128) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut bytes = [b'0'; 8];
    for index in (0..8).rev() {
        bytes[index] = ALPHABET[(value % 36) as usize];
        value /= 36;
    }
    String::from_utf8(bytes.to_vec()).expect("base36 id should be utf8")
}
