use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "dexrs", version)]
pub struct Cli {
    #[arg(long, global = true, value_name = "path")]
    pub config: Option<std::path::PathBuf>,
    #[arg(long, global = true, value_name = "path")]
    pub storage_path: Option<std::path::PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Dir {
        #[arg(long)]
        global: bool,
    },
    Init {
        #[arg(short, long)]
        yes: bool,
        #[arg(long, value_name = "PATH")]
        config_dir: Option<std::path::PathBuf>,
    },
    Config {
        input: Option<String>,
        #[arg(short, long, conflicts_with = "local")]
        global: bool,
        #[arg(short, long)]
        local: bool,
        #[arg(long)]
        unset: bool,
        #[arg(long)]
        list: bool,
    },
    #[command(alias = "add")]
    Create {
        name: Option<String>,
        #[arg(short = 'n', long = "name", conflicts_with = "name")]
        name_flag: Option<String>,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(short, long)]
        priority: Option<i64>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(short, long)]
        blocked_by: Option<String>,
    },
    Completion {
        shell: clap_complete::Shell,
    },
    Sync {
        task_id: Option<String>,
        #[arg(long, conflicts_with = "shortcut")]
        github: bool,
        #[arg(long)]
        shortcut: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Import {
        reference: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, conflicts_with = "shortcut")]
        github: bool,
        #[arg(long)]
        shortcut: bool,
        #[arg(long)]
        update: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Export {
        ids: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Archive {
        id: Option<String>,
        #[arg(long, conflicts_with_all = ["id", "older_than"])]
        completed: bool,
        #[arg(long, conflicts_with = "id")]
        older_than: Option<String>,
        #[arg(long)]
        except: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Plan {
        file: std::path::PathBuf,
        #[arg(short, long)]
        priority: Option<i64>,
        #[arg(long)]
        parent: Option<String>,
    },
    Start {
        id: String,
        #[arg(short, long)]
        force: bool,
    },
    #[command(alias = "done")]
    Complete {
        id: String,
        #[arg(short, long)]
        result: Option<String>,
        #[arg(short, long)]
        commit: Option<String>,
        #[arg(long, conflicts_with = "commit")]
        no_commit: bool,
        #[arg(short, long)]
        force: bool,
    },
    #[command(alias = "update")]
    Edit {
        id: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(short, long)]
        priority: Option<i64>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        add_blocker: Option<String>,
        #[arg(long)]
        remove_blocker: Option<String>,
        #[arg(short, long)]
        commit: Option<String>,
    },
    #[command(alias = "rm", alias = "remove")]
    Delete {
        id: String,
        #[arg(short, long)]
        force: bool,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    #[command(alias = "ls")]
    List {
        filter: Option<String>,
        #[arg(short, long)]
        all: bool,
        #[arg(short, long)]
        completed: bool,
        #[arg(short, long)]
        in_progress: bool,
        #[arg(short, long)]
        blocked: bool,
        #[arg(short, long)]
        ready: bool,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        issue: Option<i64>,
        #[arg(long)]
        commit: Option<String>,
        #[arg(short, long)]
        flat: bool,
        #[arg(short, long, conflicts_with = "filter")]
        query: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Show {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(short, long)]
        full: bool,
        #[arg(short, long)]
        expand: bool,
        #[arg(long)]
        json: bool,
    },
}
