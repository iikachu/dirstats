// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

use clap::Parser;
use dirstats::Cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("dirstats: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "png")]
    if let Some(out) = &cli.png {
        return dirstats::write_png(cli, out);
    }
    if cli.summary {
        return dirstats::print_summary(cli);
    }
    // With both front ends built, --tui picks the terminal; a TUI-only
    // build always uses it.
    #[cfg(feature = "tui")]
    if cli.tui || cfg!(not(feature = "gui")) {
        dirstats::run_tui(cli)?;
        return Ok(());
    }
    #[cfg(feature = "gui")]
    {
        dirstats::run_gui(cli)
    }
    #[cfg(not(feature = "gui"))]
    {
        dirstats::print_summary(cli)
    }
}
