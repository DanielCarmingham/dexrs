use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "dexrs")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Dir,
    Init,
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
    Start {
        id: String,
    },
    #[command(alias = "done")]
    Complete {
        id: String,
        #[arg(short, long)]
        result: Option<String>,
        #[arg(short, long)]
        commit: Option<String>,
        #[arg(long)]
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
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
}
