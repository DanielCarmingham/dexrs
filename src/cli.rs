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
        name: String,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(short, long)]
        priority: Option<i64>,
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
        #[arg(long)]
        name: Option<String>,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(short, long)]
        priority: Option<i64>,
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
