// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! The `dirstats` binary: parses the command line and hands it to the
//! `dirstats` library.

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

/// `--png` wins over `--summary`, which wins over an interface.
fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "png")]
    if let Some(out) = &cli.png {
        return dirstats::write_png(cli, out);
    }
    if cli.summary {
        return dirstats::print_summary(cli);
    }
    front_end(cli)
}

/// Picks the interface. --tui and --gui decide outright. Otherwise the
/// window is used when the session looks graphical, the terminal when it
/// does not (or a printed summary when there is no terminal either), and
/// the terminal again, if there is one, when the window then fails to
/// open, except on Windows.
#[cfg(all(feature = "gui", feature = "tui"))]
fn front_end(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    use dirstats::session;
    if cli.tui {
        return Ok(dirstats::run_tui(cli)?);
    }
    let terminal = session::is_terminal();
    if !cli.gui && !session::gui_available() {
        if terminal {
            return Ok(dirstats::run_tui(cli)?);
        }
        eprintln!("dirstats: no graphical session or terminal; printing a summary (--gui forces a window)");
        return dirstats::print_summary(cli);
    }
    match dirstats::run_gui(cli) {
        // Windows always has a desktop, so a failure there is a real error
        // to report, not a sign the session is text-only.
        Err(err) if !cli.gui && terminal && !cfg!(windows) => {
            eprintln!("dirstats: could not open a window ({err}); using the terminal interface");
            Ok(dirstats::run_tui(cli)?)
        }
        result => result,
    }
}

#[cfg(all(feature = "gui", not(feature = "tui")))]
fn front_end(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    dirstats::run_gui(cli)
}

#[cfg(all(feature = "tui", not(feature = "gui")))]
fn front_end(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    Ok(dirstats::run_tui(cli)?)
}

#[cfg(not(any(feature = "gui", feature = "tui")))]
fn front_end(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    dirstats::print_summary(cli)
}
