mod cli;
mod commands;
pub mod store;
pub mod task;

use std::ffi::OsString;
use std::io::{self, Write};

pub fn main_entry() -> anyhow::Result<()> {
    let code = run_with_io(std::env::args_os(), io::stdout(), io::stderr())?;
    std::process::exit(code);
}

pub fn run_with_io<I, W, E>(args: I, stdout: W, stderr: E) -> anyhow::Result<i32>
where
    I: IntoIterator<Item = OsString>,
    W: Write,
    E: Write,
{
    commands::run(args, stdout, stderr)
}
