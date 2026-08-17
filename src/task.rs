use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub priority: Option<String>,
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
