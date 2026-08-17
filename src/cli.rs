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
        priority: Option<String>,
    },
    Start {
        id: String,
    },
    #[command(alias = "done")]
    Complete {
        id: String,
        result: Option<String>,
        #[arg(long)]
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
        priority: Option<String>,
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
