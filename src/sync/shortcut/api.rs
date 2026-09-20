use anyhow::{Context, anyhow};
use serde_json::Value;

pub const DEFAULT_API_URL: &str = "https://api.app.shortcut.com/api/v3";

pub fn api_url() -> String {
    std::env::var("DEX_SHORTCUT_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string())
}

#[derive(Debug, Clone)]
pub struct Story {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub completed: bool,
    pub workflow_state_id: Option<u64>,
    pub labels: Vec<String>,
    pub app_url: String,
    pub story_links: Vec<StoryLink>,
}

#[derive(Debug, Clone)]
pub struct StoryLink {
    pub subject_id: u64,
    pub object_id: u64,
    pub verb: String,
}

#[derive(Debug, Clone)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub mention_name: String,
    pub workflow_ids: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct WorkflowState {
    pub id: u64,
    pub kind: String,
}

impl Story {
    fn from_value(value: &Value) -> Option<Story> {
        Some(Story {
            id: value.get("id")?.as_u64()?,
            name: value["name"].as_str().unwrap_or_default().to_string(),
            description: value["description"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            completed: value["completed"].as_bool().unwrap_or(false),
            workflow_state_id: value["workflow_state_id"].as_u64(),
            labels: value["labels"]
                .as_array()
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|label| label["name"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            app_url: value["app_url"].as_str().unwrap_or_default().to_string(),
            story_links: value["story_links"]
                .as_array()
                .map(|links| {
                    links
                        .iter()
                        .filter_map(|link| {
                            Some(StoryLink {
                                subject_id: link["subject_id"].as_u64()?,
                                object_id: link["object_id"].as_u64()?,
                                verb: link["verb"].as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

fn team_from_value(value: &Value) -> Option<Team> {
    Some(Team {
        id: value.get("id")?.as_str()?.to_string(),
        name: value["name"].as_str().unwrap_or_default().to_string(),
        mention_name: value["mention_name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        workflow_ids: value["workflow_ids"]
            .as_array()
            .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default(),
    })
}

pub struct ShortcutClient {
    base_url: String,
    token: String,
    agent: ureq::Agent,
}

impl ShortcutClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }

    pub fn workspace_slug(&self) -> anyhow::Result<String> {
        let member = self.request("GET", "/member", None)?;
        member["workspace2"]["url_slug"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Shortcut member info has no workspace slug"))
    }

    pub fn get_story(&self, id: u64) -> anyhow::Result<Story> {
        let value = self.request("GET", &format!("/stories/{id}"), None)?;
        Story::from_value(&value).ok_or_else(|| anyhow!("Shortcut returned a story without an id"))
    }

    pub fn create_story(&self, fields: Value) -> anyhow::Result<Story> {
        let value = self.request("POST", "/stories", Some(&fields))?;
        Story::from_value(&value).ok_or_else(|| anyhow!("Shortcut returned a story without an id"))
    }

    pub fn update_story(&self, id: u64, fields: Value) -> anyhow::Result<Story> {
        let value = self.request("PUT", &format!("/stories/{id}"), Some(&fields))?;
        Story::from_value(&value).ok_or_else(|| anyhow!("Shortcut returned a story without an id"))
    }

    pub fn search_stories(&self, query: &str) -> anyhow::Result<Vec<Story>> {
        let value = self.request(
            "GET",
            &format!("/search/stories?query={}&page_size=100", urlencode(query)),
            None,
        )?;
        Ok(value["data"]
            .as_array()
            .map(|stories| stories.iter().filter_map(Story::from_value).collect())
            .unwrap_or_default())
    }

    pub fn create_blocks_link(&self, blocker_id: u64, blocked_id: u64) -> anyhow::Result<()> {
        self.request(
            "POST",
            "/story-links",
            Some(&serde_json::json!({
                "subject_id": blocker_id,
                "object_id": blocked_id,
                "verb": "blocks",
            })),
        )?;
        Ok(())
    }

    pub fn workflow_states(&self, workflow_id: u64) -> anyhow::Result<Vec<WorkflowState>> {
        let value = self.request("GET", &format!("/workflows/{workflow_id}"), None)?;
        Ok(value["states"]
            .as_array()
            .map(|states| {
                states
                    .iter()
                    .filter_map(|state| {
                        Some(WorkflowState {
                            id: state["id"].as_u64()?,
                            kind: state["type"].as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn list_teams(&self) -> anyhow::Result<Vec<Team>> {
        let value = self.request("GET", "/groups", None)?;
        Ok(value
            .as_array()
            .map(|teams| teams.iter().filter_map(team_from_value).collect())
            .unwrap_or_default())
    }

    pub fn get_team(&self, id: &str) -> anyhow::Result<Team> {
        let value = self.request("GET", &format!("/groups/{id}"), None)?;
        team_from_value(&value).ok_or_else(|| anyhow!("Shortcut returned a team without an id"))
    }

    pub fn ensure_label(&self, name: &str) -> anyhow::Result<()> {
        let labels = self.request("GET", "/labels", None)?;
        let exists = labels
            .as_array()
            .is_some_and(|labels| labels.iter().any(|label| label["name"] == name));
        if !exists {
            self.request(
                "POST",
                "/labels",
                Some(&serde_json::json!({ "name": name })),
            )?;
        }
        Ok(())
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let with_body = |builder: ureq::RequestBuilder<ureq::typestate::WithBody>| {
            builder
                .header("Shortcut-Token", &self.token)
                .header("Content-Type", "application/json")
                .header("User-Agent", "dexrs")
        };
        let response = match (method, body) {
            ("GET", _) => self
                .agent
                .get(&url)
                .header("Shortcut-Token", &self.token)
                .header("User-Agent", "dexrs")
                .call(),
            ("POST", Some(body)) => with_body(self.agent.post(&url)).send_json(body),
            ("PUT", Some(body)) => with_body(self.agent.put(&url)).send_json(body),
            _ => return Err(anyhow!("unsupported Shortcut request {method} {path}")),
        };
        let mut response = response.map_err(|error| anyhow!("Shortcut request failed: {error}"))?;
        let status = response.status().as_u16();
        if status >= 400 {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value["message"].as_str().map(str::to_string))
                .unwrap_or(text);
            return Err(anyhow!(
                "Shortcut API error: HTTP {status} for {method} {path}: {message}"
            ));
        }
        if status == 204 {
            return Ok(Value::Null);
        }
        response
            .body_mut()
            .read_json()
            .context("Shortcut returned invalid JSON")
    }
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
