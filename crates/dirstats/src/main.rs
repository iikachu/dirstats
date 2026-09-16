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
    // With both front ends built, --gui picks the window; a GUI-only build
    // always opens it.
    #[cfg(feature = "gui")]
    if cli.gui || cfg!(not(feature = "tui")) {
        return dirstats::run_gui(cli);
    }
    #[cfg(feature = "tui")]
    {
        dirstats::run_tui(cli)?;
        Ok(())
    }
    #[cfg(not(feature = "tui"))]
    {
        dirstats::print_summary(cli)
    }
}
