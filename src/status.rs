use serde::Serialize;
use serde_json::json;

use crate::listing::{find, is_blocked, is_in_progress, render_tree, task_line};
use crate::task::Task;

const RECENT_LIMIT: usize = 5;
const RULE: &str = "────────────────────";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total: usize,
    pub pending: usize,
    pub completed: usize,
    pub blocked: usize,
    pub ready: usize,
    pub in_progress: usize,
}

pub struct Dashboard<'a> {
    pub stats: Stats,
    pub in_progress: Vec<&'a Task>,
    pub ready: Vec<&'a Task>,
    pub blocked: Vec<&'a Task>,
    pub recently_completed: Vec<&'a Task>,
}

pub fn dashboard(tasks: &[Task]) -> Dashboard<'_> {
    let pending: Vec<&Task> = tasks.iter().filter(|task| !task.completed).collect();
    let in_progress = sorted(pending.iter().copied().filter(|task| is_in_progress(task)));
    let blocked = sorted(
        pending
            .iter()
            .copied()
            .filter(|task| is_blocked(tasks, task) || has_open_children(tasks, task)),
    );
    let ready = sorted(pending.iter().copied().filter(|task| {
        task.started_at.is_none() && !is_blocked(tasks, task) && !has_open_children(tasks, task)
    }));

    let mut recently_completed: Vec<&Task> = tasks.iter().filter(|task| task.completed).collect();
    recently_completed.sort_by(|left, right| right.completed_at.cmp(&left.completed_at));
    recently_completed.truncate(RECENT_LIMIT);

    let stats = Stats {
        total: tasks.len(),
        pending: pending.len(),
        completed: tasks.len() - pending.len(),
        blocked: blocked.len(),
        ready: ready.len(),
        in_progress: in_progress.len(),
    };
    Dashboard {
        stats,
        in_progress,
        ready,
        blocked,
        recently_completed,
    }
}

pub fn render(tasks: &[Task], dashboard: &Dashboard<'_>) -> String {
    if tasks.is_empty() {
        return "No tasks yet. Create one with: dex create \"Task name\" --description \"Details\"\n"
            .to_string();
    }

    let stats = &dashboard.stats;
    let percent = (stats.completed * 100 + stats.total / 2) / stats.total;
    let mut output = format!(
        "{:>5}{:>9}{:>9}{:>9}   \ncomplete   active   ready   blocked\n",
        format!("{percent}%"),
        stats.in_progress,
        stats.ready,
        stats.blocked
    );

    push_section(
        &mut output,
        &format!("In Progress ({})", dashboard.in_progress.len()),
        render_tree(tasks, &dashboard.in_progress),
    );
    push_section(
        &mut output,
        &format!("Ready to Work ({})", dashboard.ready.len()),
        render_tree(tasks, &dashboard.ready),
    );
    push_section(
        &mut output,
        &format!("Blocked ({})", dashboard.blocked.len()),
        render_tree(tasks, &dashboard.blocked),
    );
    let recent: String = dashboard
        .recently_completed
        .iter()
        .map(|task| {
            format!(
                "{} ({})\n",
                task_line(tasks, task),
                relative_age(task.completed_at.as_deref())
            )
        })
        .collect();
    push_section(&mut output, "Recently Completed", recent);
    output
}

pub fn to_json(dashboard: &Dashboard<'_>) -> serde_json::Value {
    json!({
        "stats": dashboard.stats,
        "inProgressTasks": dashboard.in_progress,
        "readyTasks": dashboard.ready,
        "blockedTasks": dashboard.blocked,
        "recentlyCompleted": dashboard.recently_completed,
    })
}

fn push_section(output: &mut String, title: &str, body: String) {
    if body.is_empty() {
        return;
    }
    output.push_str(&format!("\n{title}\n{RULE}\n{body}"));
}

fn has_open_children(tasks: &[Task], task: &Task) -> bool {
    task.children
        .iter()
        .any(|id| find(tasks, id).is_some_and(|child| !child.completed))
}

fn sorted<'a>(tasks: impl Iterator<Item = &'a Task>) -> Vec<&'a Task> {
    let mut tasks: Vec<&Task> = tasks.collect();
    tasks.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    tasks
}

fn relative_age(timestamp: Option<&str>) -> String {
    let parsed = timestamp.and_then(|value| {
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
    });
    let Some(then) = parsed else {
        return "unknown".to_string();
    };
    let elapsed = time::OffsetDateTime::now_utc() - then;
    let minutes = elapsed.whole_minutes().max(0);
    if minutes < 60 {
        format!("{minutes}m ago")
    } else if minutes < 60 * 24 {
        format!("{}h ago", minutes / 60)
    } else {
        format!("{}d ago", minutes / (60 * 24))
    }
}
