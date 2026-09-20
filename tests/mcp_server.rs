use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

struct Mcp {
    child: std::process::Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(store: &std::path::Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_dexrs"))
            .arg("mcp")
            .env("DEX_STORAGE_PATH", store)
            .env("DEX_HOME", store.join("dex-home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut child = child;
        let reader = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            reader,
            next_id: 1,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        let response: Value =
            serde_json::from_str(&line).unwrap_or_else(|_| panic!("bad response: {line}"));
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        response
    }

    fn notify(&mut self, method: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "method": method})).unwrap();
        stdin.flush().unwrap();
    }

    fn call(&mut self, tool: &str, arguments: Value) -> (Value, bool) {
        let response = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        let result = &response["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{response}"));
        assert_eq!(result["content"][0]["type"], "text");
        (
            serde_json::from_str(text).unwrap(),
            result["isError"].as_bool().unwrap_or(false),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn mcp_handshake_lists_the_three_tools() {
    let temp = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(&temp.path().join("store"));

    let init = mcp.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init["result"]["serverInfo"]["name"], "dex");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    mcp.notify("notifications/initialized");

    let tools = mcp.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["create_task", "update_task", "list_tasks"]);
    let create = &tools["result"]["tools"][0];
    assert_eq!(create["inputSchema"]["type"], "object");
    assert_eq!(
        create["inputSchema"]["required"],
        json!(["name", "description"])
    );
    assert!(create["inputSchema"]["properties"]["blocked_by"]["items"].is_object());
    assert!(
        create["description"]
            .as_str()
            .unwrap()
            .contains("GitHub Issue")
    );
    let update = &tools["result"]["tools"][1];
    assert_eq!(update["inputSchema"]["required"], json!(["id"]));
    assert_eq!(
        update["inputSchema"]["properties"]["commit_sha"]["pattern"],
        "^[0-9a-f]{7,40}$"
    );

    assert_eq!(mcp.request("ping", json!({}))["result"], json!({}));
    let unknown = mcp.request("nope/method", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn mcp_tools_create_update_list_and_delete_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    let mut mcp = Mcp::start(&store);

    let (parent, is_error) = mcp.call(
        "create_task",
        json!({"name": "Parent", "description": "Parent body", "priority": 2}),
    );
    assert!(!is_error, "{parent}");
    let parent_id = parent["id"].as_str().unwrap().to_string();
    assert_eq!(parent["priority"], 2);
    assert_eq!(parent["description"], "Parent body");

    let (child, _) = mcp.call(
        "create_task",
        json!({"name": "Child", "description": "Child body", "parent_id": parent_id, "blocked_by": []}),
    );
    let child_id = child["id"].as_str().unwrap().to_string();
    assert_eq!(child["parent_id"], parent_id);

    let (listed, _) = mcp.call("list_tasks", json!({}));
    assert_eq!(listed.as_array().unwrap().len(), 2);
    let (searched, _) = mcp.call("list_tasks", json!({"query": "child"}));
    assert_eq!(searched.as_array().unwrap().len(), 1);
    assert_eq!(searched[0]["id"], child_id);

    let (updated, is_error) = mcp.call(
        "update_task",
        json!({
            "id": child_id,
            "completed": true,
            "result": "Done it",
            "commit_sha": "abc1234",
            "commit_message": "feat: child",
            "started_at": "2026-01-01T00:00:00Z",
        }),
    );
    assert!(!is_error, "{updated}");
    assert_eq!(updated["completed"], true);
    assert!(updated["completed_at"].is_string());
    assert_eq!(updated["result"], "Done it");
    assert_eq!(updated["metadata"]["commit"]["sha"], "abc1234");
    assert_eq!(updated["metadata"]["commit"]["message"], "feat: child");
    assert!(updated["metadata"]["commit"]["timestamp"].is_string());
    assert_eq!(updated["started_at"], "2026-01-01T00:00:00Z");

    let (pending, _) = mcp.call("list_tasks", json!({}));
    assert_eq!(pending.as_array().unwrap().len(), 1);
    let (completed, _) = mcp.call("list_tasks", json!({"completed": true}));
    assert_eq!(completed[0]["id"], child_id);
    let (all, _) = mcp.call("list_tasks", json!({"all": true}));
    assert_eq!(all.as_array().unwrap().len(), 2);

    let (deleted, is_error) = mcp.call("update_task", json!({"id": parent_id, "delete": true}));
    assert!(!is_error, "{deleted}");
    assert_eq!(deleted["deleted"], true);
    assert_eq!(deleted["id"], parent_id);
    assert_eq!(deleted["task"]["name"], "Parent");
    assert!(dexrs::store::read_tasks(&store).unwrap().is_empty());
}

#[test]
fn mcp_reports_validation_and_lookup_errors_in_content() {
    let temp = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(&temp.path().join("store"));

    let (error, is_error) = mcp.call("create_task", json!({"description": "no name"}));
    assert!(is_error);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .starts_with("Validation error: name:"),
        "{error}"
    );

    let (error, is_error) = mcp.call("update_task", json!({"id": "missing1", "name": "x"}));
    assert!(is_error);
    assert_eq!(error["error"], "Task \"missing1\" not found");
    assert_eq!(
        error["suggestion"],
        "Run \"dex list --all\" to see all available tasks"
    );

    let (error, is_error) = mcp.call(
        "update_task",
        json!({"id": "missing1", "commit_sha": "xyz"}),
    );
    assert!(is_error);
    assert!(
        error["error"].as_str().unwrap().contains("commit_sha"),
        "{error}"
    );

    let (error, is_error) = mcp.call("nonexistent_tool", json!({}));
    assert!(is_error);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("nonexistent_tool"),
        "{error}"
    );
}
