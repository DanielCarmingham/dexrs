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
}
