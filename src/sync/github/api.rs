use anyhow::{Context, anyhow};
use serde_json::Value;

pub const DEFAULT_API_URL: &str = "https://api.github.com";

pub fn api_url() -> String {
    std::env::var("DEX_GITHUB_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string())
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub html_url: String,
    pub labels: Vec<String>,
    pub is_pull_request: bool,
}

impl Issue {
    fn from_value(value: &Value) -> Option<Issue> {
        Some(Issue {
            number: value.get("number")?.as_u64()?,
            title: value["title"].as_str().unwrap_or_default().to_string(),
            body: value["body"].as_str().unwrap_or_default().to_string(),
            state: value["state"].as_str().unwrap_or("open").to_string(),
            html_url: value["html_url"].as_str().unwrap_or_default().to_string(),
            labels: value["labels"]
                .as_array()
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|label| match label {
                            Value::String(name) => Some(name.clone()),
                            other => other["name"].as_str().map(str::to_string),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            is_pull_request: value.get("pull_request").is_some_and(|pr| !pr.is_null()),
        })
    }
}

pub struct GitHubClient {
    base_url: String,
    token: String,
    agent: ureq::Agent,
}

impl GitHubClient {
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

    pub fn get_issue(&self, owner: &str, repo: &str, number: u64) -> anyhow::Result<Issue> {
        let value = self.request(
            "GET",
            &format!("/repos/{owner}/{repo}/issues/{number}"),
            None,
        )?;
        Issue::from_value(&value)
            .ok_or_else(|| anyhow!("GitHub returned an issue without a number"))
    }

    pub fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: &str,
        labels: &[String],
    ) -> anyhow::Result<Issue> {
        let payload = serde_json::json!({ "title": title, "body": body, "labels": labels });
        let value = self.request(
            "POST",
            &format!("/repos/{owner}/{repo}/issues"),
            Some(&payload),
        )?;
        Issue::from_value(&value)
            .ok_or_else(|| anyhow!("GitHub returned an issue without a number"))
    }

    pub fn update_issue(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        fields: Value,
    ) -> anyhow::Result<Issue> {
        let value = self.request(
            "PATCH",
            &format!("/repos/{owner}/{repo}/issues/{number}"),
            Some(&fields),
        )?;
        Issue::from_value(&value)
            .ok_or_else(|| anyhow!("GitHub returned an issue without a number"))
    }

    pub fn create_comment(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        body: &str,
    ) -> anyhow::Result<()> {
        self.request(
            "POST",
            &format!("/repos/{owner}/{repo}/issues/{number}/comments"),
            Some(&serde_json::json!({ "body": body })),
        )?;
        Ok(())
    }

    /// Lists every issue carrying `label`, following `Link: rel="next"` pages.
    pub fn list_issues(&self, owner: &str, repo: &str, label: &str) -> anyhow::Result<Vec<Issue>> {
        let mut url = format!(
            "{}/repos/{owner}/{repo}/issues?labels={}&state=all&per_page=100",
            self.base_url,
            urlencode(label)
        );
        let mut issues = Vec::new();
        loop {
            let response = self
                .agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "dexrs")
                .call();
            let mut response =
                response.map_err(|error| anyhow!("GitHub request failed: {error}"))?;
            if response.status().as_u16() >= 400 {
                return Err(api_error(&mut response, "GET", &url));
            }
            let next = response
                .headers()
                .get("link")
                .and_then(|value| value.to_str().ok())
                .and_then(next_link);
            let page: Value = response
                .body_mut()
                .read_json()
                .context("GitHub returned invalid JSON")?;
            if let Some(items) = page.as_array() {
                issues.extend(items.iter().filter_map(Issue::from_value));
            }
            match next {
                Some(next_url) => url = next_url,
                None => return Ok(issues),
            }
        }
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let request = |builder: ureq::RequestBuilder<ureq::typestate::WithBody>| {
            builder
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "dexrs")
        };
        let response = match (method, body) {
            ("GET", _) => self
                .agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "dexrs")
                .call(),
            ("POST", Some(body)) => request(self.agent.post(&url)).send_json(body),
            ("PATCH", Some(body)) => request(self.agent.patch(&url)).send_json(body),
            _ => return Err(anyhow!("unsupported GitHub request {method} {path}")),
        };
        let mut response = response.map_err(|error| anyhow!("GitHub request failed: {error}"))?;
        if response.status().as_u16() >= 400 {
            return Err(api_error(&mut response, method, path));
        }
        response
            .body_mut()
            .read_json()
            .context("GitHub returned invalid JSON")
    }
}

fn api_error(
    response: &mut ureq::http::Response<ureq::Body>,
    method: &str,
    target: &str,
) -> anyhow::Error {
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().unwrap_or_default();
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| value["message"].as_str().map(str::to_string))
        .unwrap_or(body);
    anyhow!("GitHub API error: HTTP {status} for {method} {target}: {message}")
}

fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
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
