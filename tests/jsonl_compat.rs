use dexrs::task::{parse_tasks_jsonl, serialize_tasks_jsonl};

#[test]
fn parses_dex_compatible_jsonl_fixture() {
    let input = include_str!("fixtures/dex-tasks.jsonl");

    let tasks = parse_tasks_jsonl(input).unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "abc123xy");
    assert_eq!(tasks[0].parent_id, None);
    assert_eq!(tasks[0].name, "Parent task");
    assert_eq!(tasks[0].description.as_deref(), Some("Top-level work"));
    assert_eq!(tasks[0].priority.as_deref(), Some("high"));
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
