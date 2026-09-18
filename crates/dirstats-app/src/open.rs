// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Opening entries with the desktop's default handler.

use crate::{App, NodeId};
use std::io;

impl App {
    /// Open the selected entry with the desktop's default handler.
    pub fn open_selected(&mut self) -> io::Result<()> {
        let id = self.selected().ok_or(io::ErrorKind::NotFound)?;
        self.open_node(id)
    }

    /// Open `id` with the desktop's default handler.
    pub fn open_node(&mut self, id: NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        open::that_detached(&path)?;
        self.message = Some(format!("opened {}", path.display()));
        Ok(())
    }
}
