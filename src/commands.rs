use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

use crate::cli::{Cli, Command};

pub fn run<I, W, E>(args: I, mut stdout: W, _stderr: E) -> anyhow::Result<i32>
where
    I: IntoIterator<Item = OsString>,
    W: Write,
    E: Write,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Dir => {
            let store = std::env::var_os("DEX_STORAGE_PATH")
                .map(PathBuf::from)
                .context("DEX_STORAGE_PATH is required until store discovery is implemented")?;
            writeln!(stdout, "{}", store.display())?;
            Ok(0)
        }
    }
}
