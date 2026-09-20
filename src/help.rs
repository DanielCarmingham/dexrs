pub const COMMANDS: &[&str] = &[
    "create",
    "list",
    "ls",
    "show",
    "edit",
    "update",
    "complete",
    "done",
    "delete",
    "rm",
    "remove",
    "archive",
    "plan",
    "sync",
    "import",
    "export",
    "doctor",
    "status",
    "config",
    "help",
    "mcp",
    "completion",
];

pub fn suggestion(input: &str) -> Option<&'static str> {
    let input = input.to_lowercase();
    COMMANDS
        .iter()
        .map(|command| (levenshtein(&input, command), *command))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, command)| command)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current.push(
                (previous[j] + cost)
                    .min(previous[j + 1] + 1)
                    .min(current[j] + 1),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

pub fn text(name: &str) -> String {
    format!(
        r#"Task tracking tool

USAGE:
  {name} <command> [options]

COMMANDS:
  init                             Create config file (~/.config/dex/dex.toml)
  config <key>[=<value>]           Get or set config values
  dir                              Print task storage directory
  dir --global                     Print global dex config directory
  mcp                              Start MCP server (stdio)
  status                           Show dashboard overview (default)
  create "name" [--description "..."]  Create task
  add                                   Alias for create command
  list, ls                         List all pending tasks (tree view)
  list --flat                      List without tree hierarchy
  list --all                       Include completed tasks
  list --archived                  List archived tasks
  list --in-progress               List only in-progress tasks
  list --query "login"             Search name/description
  list --json                      Output as JSON (for scripts)
  show <id>...                     View task details (truncated)
  show <id> --expand               Show ancestor descriptions in tree
  show <id> --full                 View full description and result
  show <id> --json                 Output as JSON (for scripts)
  edit <id> [-n "..."]             Edit task
  edit <id> --commit <sha>         Link commit to completed task
  update                           Alias for edit command
  start <id>                       Mark task as in progress
  start <id> --force               Re-claim task already in progress
  complete <id> --result "..." [--commit <sha>|--no-commit]
                                     Mark completed with result
  complete <id> ... --force        Bypass validation (e.g., incomplete subtasks)
  done                             Alias for complete command
  delete <id>                      Remove task (refuses if has subtasks)
  delete <id> -f                   Force delete including subtasks
  rm, remove                       Aliases for delete command
  archive <id>                     Archive completed task to reduce storage
  archive --older-than 60d         Archive tasks completed >60 days ago
  archive --completed              Archive ALL completed tasks
  plan <file>                      Create task from plan markdown file
  sync [id]                        Push tasks to GitHub/Shortcut
  sync --github                    Sync only to GitHub Issues
  sync --shortcut                  Sync only to Shortcut Stories
  import #N                        Import GitHub issue
  import sc#N                      Import Shortcut story
  import --all                     Import all dex-labeled items
  export <id>...                   Export tasks to GitHub (no sync back)
  doctor [--fix]                   Check and repair configuration and storage
  completion <shell>               Generate shell completion script

GLOBAL OPTIONS:
  --version, -V                    Show version and exit
  --help, -h                       Show this help message
  --config <path>                  Use custom config file
  --storage-path <path>            Override storage file location

COMMAND OPTIONS:
  -p, --priority <n>               Task priority (lower = higher priority)
  --parent <id>                    Parent task (creates subtask)
  --json                           Output as JSON (list, show)

EXAMPLES:
  # Create with detailed description (requirements, approach, done criteria):
  {name} create "Add user auth" --description "Requirements:
    - JWT with refresh tokens
    - bcrypt for passwords
    Approach: /login, /register endpoints
    Done when: users can register/login, tests pass"

  # Create simple task (description optional):
  {name} create "Fix login bug"

  # Complete with detailed result and commit link:
  {name} complete abc123 --result "Added JWT auth:
    - /login, /register, /logout endpoints
    - bcrypt cost=12, 15min access tokens
    Decisions: JWT over sessions for scaling
    Follow-up: add email verification" --commit a1b2c3d

  # Complete without code changes (issue stays open):
  {name} complete abc123 --result "Planning complete" --no-commit

  # Create task from planning session:
  {name} plan ~/.claude/plans/my-plan.md

  # Other common operations:
  {name} list --json | jq '.[] | .id'
  {name} create "Subtask" --description "..." --parent abc123

  # Sync tasks to external services:
  {name} sync                          # Sync to all configured services
  {name} sync --github                 # Sync only to GitHub
  {name} sync --shortcut               # Sync only to Shortcut
  {name} sync --dry-run                # Preview what would be synced

  # Import from external services:
  {name} import #42                    # Import GitHub issue #42
  {name} import sc#123                 # Import Shortcut story #123
  {name} import --all                  # Import all dex-labeled items
  {name} import --all --shortcut       # Import only from Shortcut
"#
    )
}
