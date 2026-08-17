use std::ffi::OsString;
use std::io::Write;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::store;

pub fn run<I, W, E>(args: I, mut stdout: W, _stderr: E) -> anyhow::Result<i32>
where
    I: IntoIterator<Item = OsString>,
    W: Write,
    E: Write,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Dir => {
            let cwd = std::env::current_dir()?;
            let store =
                store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())?;
            writeln!(stdout, "{}", store.display())?;
            Ok(0)
        }
        Command::Init => {
            let cwd = std::env::current_dir()?;
            let store =
                store::resolve_store_dir(&cwd, std::env::var_os("DEX_STORAGE_PATH").as_deref())?;
            store::init_store(&store)?;
            writeln!(stdout, "initialized {}", store.display())?;
            Ok(0)
        }
    }
}
