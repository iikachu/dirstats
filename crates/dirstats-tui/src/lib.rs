// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// The keyboard model follows dua-cli's interactive mode (MIT, by Sebastian
// Thiel); the code here is written independently.

//! Terminal user interface: a size-sorted entry list beside a cell treemap.

// The treemap icon as an inline SVG; regenerate with
// `cargo run -p dirstats-treemap --example icon -- OUT_DIR` (dirstats-logo.url).
#![doc(
    html_logo_url = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cdefs%3E%3CradialGradient id='c' cx='.3' cy='.3' r='.9'%3E%3Cstop offset='0' stop-color='%23fff' stop-opacity='.3'/%3E%3Cstop offset='.65' stop-color='%23fff' stop-opacity='0'/%3E%3Cstop offset='1' stop-color='%23000' stop-opacity='.12'/%3E%3C/radialGradient%3E%3CclipPath id='k'%3E%3Crect x='6.8' y='6.8' width='86.4' height='86.4' rx='10.8'/%3E%3C/clipPath%3E%3C/defs%3E%3Crect x='3' y='3' width='94' height='94' rx='13.5' fill='%231f1f1f'/%3E%3Cg clip-path='url(%23k)'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3' fill='%2300d6ce'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7' fill='%2300d6ce'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7' fill='%2300d6ce'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1' fill='%23ffa062'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8' fill='%23ffa062'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2' fill='%23ffa062'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8' fill='%23ffa062'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3' fill='%23ffa062'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4' fill='%2300d6ce'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2' fill='%23a6b5ff'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3' fill='%23ff95b7'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3' fill='%2379d652'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5' fill='%2379d652'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5' fill='%2379d652'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9' fill='%2379d652'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9' fill='%2379d652'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4' fill='%2379d652'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5' fill='%2379d652'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9' fill='%2379d652'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8' fill='%2379d652'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8' fill='%23ff95b7'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7' fill='%23ff95b7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1' fill='%23ff95b7'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8' fill='%23ff95b7'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8' fill='%23ff95b7'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8' fill='%23ff95b7'/%3E%3Cg fill='url(%23c)' stroke='%234d4d4d' stroke-width='.35' stroke-opacity='.6'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8'/%3E%3C/g%3E%3C/g%3E%3C/svg%3E",
    html_favicon_url = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cdefs%3E%3CradialGradient id='c' cx='.3' cy='.3' r='.9'%3E%3Cstop offset='0' stop-color='%23fff' stop-opacity='.3'/%3E%3Cstop offset='.65' stop-color='%23fff' stop-opacity='0'/%3E%3Cstop offset='1' stop-color='%23000' stop-opacity='.12'/%3E%3C/radialGradient%3E%3CclipPath id='k'%3E%3Crect x='6.8' y='6.8' width='86.4' height='86.4' rx='10.8'/%3E%3C/clipPath%3E%3C/defs%3E%3Crect x='3' y='3' width='94' height='94' rx='13.5' fill='%231f1f1f'/%3E%3Cg clip-path='url(%23k)'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3' fill='%2300d6ce'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7' fill='%2300d6ce'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7' fill='%2300d6ce'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1' fill='%23ffa062'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8' fill='%23ffa062'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2' fill='%23ffa062'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8' fill='%23ffa062'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3' fill='%23ffa062'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4' fill='%2300d6ce'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2' fill='%23a6b5ff'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3' fill='%23ff95b7'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3' fill='%2379d652'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5' fill='%2379d652'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5' fill='%2379d652'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9' fill='%2379d652'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9' fill='%2379d652'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4' fill='%2379d652'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5' fill='%2379d652'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9' fill='%2379d652'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8' fill='%2379d652'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8' fill='%23ff95b7'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7' fill='%23ff95b7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1' fill='%23ff95b7'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8' fill='%23ff95b7'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8' fill='%23ff95b7'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8' fill='%23ff95b7'/%3E%3Cg fill='url(%23c)' stroke='%234d4d4d' stroke-width='.35' stroke-opacity='.6'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8'/%3E%3C/g%3E%3C/g%3E%3C/svg%3E"
)]

mod treemap;

use dirstats_core::App;
use dirstats_core::format;
use dirstats_core::treemap::ExtensionColors;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::io;
use std::time::Duration;

/// How long to wait for a key before redrawing, so scan progress keeps moving.
const TICK: Duration = Duration::from_millis(100);

/// Run the interface until the user quits. Sets up and restores the terminal.
pub fn run(app: &mut App) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, app);
    ratatui::restore();
    result
}

/// Poll the app, redraw and handle one key per tick until a quit key.
fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    // Extension ranking is per tree, so it is rebuilt only when a scan lands.
    let mut colors: Option<ExtensionColors> = None;
    loop {
        if app.poll() || (colors.is_none() && app.tree.is_some()) {
            colors = app.tree.as_ref().map(ExtensionColors::rank);
        }
        terminal.draw(|frame| draw(frame, app, colors.as_ref()))?;
        if !event::poll(TICK)? {
            continue;
        }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('c') if ctrl => return Ok(()),
            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char('d') => {
                app.move_selection(10);
            }
            KeyCode::PageUp | KeyCode::Char('u') => app.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => app.select_first(),
            KeyCode::End | KeyCode::Char('G') => app.select_last(),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                app.enter();
            }
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                app.back();
            }
            #[cfg(feature = "open")]
            KeyCode::Char('o') => {
                if let Err(err) = app.open_selected() {
                    app.message = Some(format!("open failed: {err}"));
                }
            }
            #[cfg(feature = "trash")]
            KeyCode::Char('x') if ctrl => {
                if let Err(err) = app.trash_selected() {
                    app.message = Some(format!("trash failed: {err}"));
                }
            }
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, app: &App, colors: Option<&ExtensionColors>) {
    let [header, body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .areas(frame.area());
    let [list_area, map_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .areas(body);

    frame.render_widget(Paragraph::new(header_line(app)), header);
    draw_list(frame, app, list_area);
    draw_map(frame, app, colors, map_area);
    frame.render_widget(Paragraph::new(footer_line(app)), footer);
}

fn header_line(app: &App) -> Line<'static> {
    let Some(tree) = &app.tree else {
        return Line::from(" dirstats");
    };
    let crumbs = app.breadcrumbs();
    let mut spans = vec![Span::raw(" ")];
    for (i, &id) in crumbs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" › ", Style::default().fg(Color::DarkGray)));
        }
        let name = tree.node(id).name.to_string_lossy().into_owned();
        let style = if i + 1 == crumbs.len() {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(name, style));
    }
    if let Some(&dir) = crumbs.last() {
        spans.push(Span::styled(
            format!("   {}  ·  {} files", format::size(tree.size(dir)), tree.node(dir).file_count),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

fn footer_line(app: &App) -> Line<'static> {
    if let Some(scan) = &app.scan {
        return Line::from(Span::styled(
            format!(
                " scanning {}  {} entries, {} skipped, {:.1}s   q quit",
                scan.root.display(),
                scan.entries(),
                scan.errors(),
                scan.started.elapsed().as_secs_f64()
            ),
            Style::default().fg(Color::Yellow),
        ));
    }
    if let Some(message) = &app.message {
        return Line::from(Span::styled(format!(" {message}"), Style::default().fg(Color::Cyan)));
    }
    let mut keys = String::from(" ↑↓/jk move  ⏎/l enter  ⌫/h back  g/G ends");
    if cfg!(feature = "open") {
        keys.push_str("  o open");
    }
    if cfg!(feature = "trash") {
        keys.push_str("  ^x trash");
    }
    keys.push_str("  q quit");
    Line::from(Span::styled(keys, Style::default().fg(Color::DarkGray)))
}

/// The current directory's entries, largest first, with size, share bar and
/// percentage of the directory, or a waiting line before the first scan lands.
fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::RIGHT);
    let Some(tree) = &app.tree else {
        frame.render_widget(Paragraph::new(" waiting for scan…").block(block), area);
        return;
    };
    let Some(cursor) = &app.cursor else { return };
    let total = tree.size(cursor.dir);
    let bar_width = 10usize;
    let name_width = (area.width as usize).saturating_sub(12 + bar_width + 8).max(8);

    let items: Vec<ListItem> = app
        .entries()
        .iter()
        .map(|&id| {
            let node = tree.node(id);
            let size = tree.size(id);
            let share = format::percent(size, total);
            let filled = ((share / 100.0) * bar_width as f64).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
            let mut name = node.name.to_string_lossy().into_owned();
            if !tree.children(id).is_empty() {
                name.push('/');
            }
            let name: String = name.chars().take(name_width).collect();
            let color = match node.kind {
                dirstats_core::scan::Kind::Directory => Color::Blue,
                dirstats_core::scan::Kind::Symlink => Color::Magenta,
                _ => Color::Reset,
            };
            let mut spans = vec![
                Span::styled(format!("{:>10} ", format::size(size)), Style::default().fg(Color::Green)),
                Span::styled(bar, Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" {share:>5.1}% "), Style::default().fg(Color::DarkGray)),
                Span::styled(name, Style::default().fg(color)),
            ];
            if node.error {
                spans.push(Span::styled(" !", Style::default().fg(Color::Red)));
            }
            if node.duplicate_link {
                spans.push(Span::styled(" (hard link)", Style::default().fg(Color::DarkGray)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(cursor.selected));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_map(frame: &mut Frame, app: &App, colors: Option<&ExtensionColors>, area: Rect) {
    let (Some(tree), Some(cursor), Some(colors)) = (&app.tree, &app.cursor, colors) else { return };
    treemap::render(frame.buffer_mut(), tree, colors, cursor.dir, app.selected(), area);
}
