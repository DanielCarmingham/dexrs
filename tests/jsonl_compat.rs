use dexrs::task::{parse_tasks_jsonl, serialize_tasks_jsonl};
use dexrs::validate::{validate_completion, validate_tasks};

#[test]
fn parses_dex_compatible_jsonl_fixture() {
    let input = include_str!("fixtures/dex-tasks.jsonl");

    let tasks = parse_tasks_jsonl(input).unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "abc123xy");
    assert_eq!(tasks[0].parent_id, None);
    assert_eq!(tasks[0].name, "Parent task");
    assert_eq!(tasks[0].description.as_deref(), Some("Top-level work"));
    assert_eq!(
        tasks[0]
            .priority
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("high")
    );
    assert!(!tasks[0].completed);
    assert_eq!(tasks[0].blocks, vec!["def456uv"]);
    assert_eq!(tasks[0].children, vec!["def456uv"]);
    assert_eq!(
        tasks[0].metadata.as_ref().unwrap()["unknown"]["nested"],
        serde_json::json!(true)
    );

    assert_eq!(tasks[1].parent_id.as_deref(), Some("abc123xy"));
    assert!(tasks[1].completed);
    assert_eq!(tasks[1].result.as_deref(), Some("done"));
    assert_eq!(tasks[1].blocked_by, vec!["abc123xy"]);
}

#[test]
fn parses_original_dex_numeric_priority() {
    let input = r#"{"id":"6gzsamjl","parent_id":null,"name":"test","description":"more description details","priority":1,"completed":false,"result":null,"metadata":null,"created_at":"2026-08-18T07:23:06.600Z","updated_at":"2026-08-18T07:23:06.600Z","started_at":null,"completed_at":null,"blockedBy":[],"blocks":[],"children":[]}"#;

    let tasks = parse_tasks_jsonl(input).unwrap();
    let output = serialize_tasks_jsonl(&tasks).unwrap();

    assert_eq!(tasks.len(), 1);
    assert!(output.contains(r#""priority":1"#));
}

#[test]
fn serializes_tasks_as_stable_jsonl_ordered_by_id() {
    let input = include_str!("fixtures/dex-tasks.jsonl");
    let mut tasks = parse_tasks_jsonl(input).unwrap();
    tasks.reverse();

    let output = serialize_tasks_jsonl(&tasks).unwrap();

    assert!(output.ends_with('\n'));
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains(r#""id":"abc123xy""#));
    assert!(lines[1].contains(r#""id":"def456uv""#));
    assert!(lines[0].contains(r#""blockedBy":[]"#));
}

#[test]
fn validate_rejects_missing_parent() {
    let mut tasks = parse_tasks_jsonl(include_str!("fixtures/dex-tasks.jsonl")).unwrap();
    tasks[0].children.clear();
    tasks[1].parent_id = Some("missing1".to_string());

    let error = validate_tasks(&tasks).unwrap_err().to_string();

    assert!(error.contains("missing parent"));
}

#[test]
fn validate_rejects_mismatched_children() {
    let mut tasks = parse_tasks_jsonl(include_str!("fixtures/dex-tasks.jsonl")).unwrap();
    tasks[0].children.clear();

    let error = validate_tasks(&tasks).unwrap_err().to_string();

    assert!(error.contains("children missing child def456uv"));
}

#[test]
fn validate_rejects_mismatched_blockers() {
    let mut tasks = parse_tasks_jsonl(include_str!("fixtures/dex-tasks.jsonl")).unwrap();
    tasks[0].blocks.clear();

    let error = validate_tasks(&tasks).unwrap_err().to_string();

    assert!(error.contains("blocks missing def456uv"));
}

#[test]
fn validate_rejects_blocking_cycles() {
    let mut tasks = parse_tasks_jsonl(include_str!("fixtures/dex-tasks.jsonl")).unwrap();
    tasks[0].blocked_by.push("def456uv".to_string());
    tasks[1].blocks.push("abc123xy".to_string());

    let error = validate_tasks(&tasks).unwrap_err().to_string();

    assert!(error.contains("blocking cycle"));
}

#[test]
fn validate_completion_rejects_incomplete_children_without_force() {
    let mut tasks = parse_tasks_jsonl(include_str!("fixtures/dex-tasks.jsonl")).unwrap();
    tasks[1].completed = false;

    let error = validate_completion(&tasks, "abc123xy", false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("incomplete child def456uv"));
    validate_completion(&tasks, "abc123xy", true).unwrap();
}
