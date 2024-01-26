// Copyright (c) 2023 the Hearth contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This file is part of Hearth.
//
// Hearth is free software: you can redistribute it and/or modify it under the
// terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// Hearth is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.
//
// You should have received a copy of the GNU Affero General Public License
// along with Hearth. If not, see <https://www.gnu.org/licenses/>.

use std::collections::HashMap;

use hearth_guest::{terminal::TerminalState, Color, Mailbox, Permissions, Signal};
use kindling_host::prelude::*;
use kindling_schema::panel::{PanelEvent, PanelManagerRequest, PanelTransform};

hearth_guest::export_metadata!();

#[no_mangle]
pub extern "C" fn run() {
    // function to transform a terminal panel at the given grid coordinates
    let make_transform = |x, y| PanelTransform {
        position: (x as f32 * 2.8 - 1.4, y as f32 * 2.8 - 1.4, 0.0).into(),
        orientation: Default::default(),
        half_size: (1.25, 1.25).into(),
    };

    // create a list of each terminal to spawn
    let terminal_configs = [
        (make_transform(0, 0), Palette::rose_pine()),
        (make_transform(0, 1), Palette::gruvbox_material()),
        (make_transform(1, 0), Palette::solarized_dark()),
        (make_transform(1, 1), Palette::pretty_in_pink()),
    ];

    // spawn each terminal using the terminal factory and a select palette
    let mut terms: Vec<_> = terminal_configs
        .iter()
        .map(|(transform, palette)| {
            let state = TerminalState {
                position: transform.position,
                orientation: transform.orientation,
                half_size: transform.half_size,
                opacity: 0.8,
                padding: Default::default(),
                units_per_em: 0.06,
                colors: palette.to_ansi(),
            };

            (Terminal::new(state.clone()), state)
        })
        .collect();

    sleep(0.1);

    let panels = REGISTRY
        .get_service("rs.hearth.kindling.PanelManager")
        .unwrap();

    let mut term_panels = Vec::new();

    // enter and execute the pipes command in each terminal
    for ((term, _state), (transform, _)) in terms.iter().zip(terminal_configs.iter()) {
        term.input("pipes\n".into());

        let on_event = Mailbox::new();

        panels.send(
            &PanelManagerRequest::CreatePanel {
                transform: *transform,
            },
            &[&on_event.make_capability(Permissions::SEND | Permissions::MONITOR)],
        );

        term_panels.push(on_event);
    }

    loop {
        let (idx, sig) = Mailbox::poll(term_panels.iter().collect::<Vec<_>>().as_slice());
        let (ref mut term, ref mut state) = terms.get_mut(idx).unwrap();

        let Signal::Message(msg) = sig else {
            panic!("expected message");
        };

        let event: PanelEvent = serde_json::from_slice(&msg.data).unwrap();

        match event {
            PanelEvent::FocusGained => {
                state.opacity = 1.0;
                term.update(state.clone());
            }
            PanelEvent::FocusLost => {
                state.opacity = 0.8;
                term.update(state.clone());
            }
            _ => {}
        }
    }
}

/// Helper struct for containing and identifying terminal colors.
struct Palette {
    pub bg: Color,
    pub fg: Color,
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
}

/// Shorthand color initialization. Fixes alpha to 0xff.
fn c(rgb: u32) -> Color {
    Color(0xff000000 | rgb)
}

impl Palette {
    /// Convert a palette into a standard terminal color map.
    pub fn to_ansi(&self) -> HashMap<usize, Color> {
        FromIterator::from_iter([
            (0x0, self.black),   // black
            (0x1, self.red),     // red
            (0x2, self.green),   // green
            (0x3, self.yellow),  // yellow
            (0x4, self.blue),    // blue
            (0x5, self.magenta), // magenta
            (0x6, self.cyan),    // cyan
            (0x7, self.white),   // white
            (0x8, self.black),   // bright black
            (0x9, self.red),     // bright red
            (0xA, self.green),   // bright green
            (0xB, self.yellow),  // bright yellow
            (0xC, self.blue),    // bright blue
            (0xD, self.magenta), // bright magenta
            (0xE, self.cyan),    // bright cyan
            (0xF, self.white),   // bright white
            (0x100, self.fg),    // foreground
            (0x101, self.bg),    // background
        ])
    }

    pub fn rose_pine() -> Self {
        Self {
            bg: c(0x191724),
            fg: c(0xe0def4),
            black: c(0x26233a),
            red: c(0xeb6f92),
            green: c(0x31748f),
            yellow: c(0xf6c177),
            blue: c(0x9ccfd8),
            magenta: c(0xc4a7e7),
            cyan: c(0xebbcba),
            white: c(0xe0def4),
        }
    }

    pub fn gruvbox_material() -> Self {
        Self {
            bg: c(0x1d2021),
            fg: c(0xd4be98),
            black: c(0x504945),
            red: c(0xea6962),
            green: c(0xa9b665),
            yellow: c(0xd8a657),
            blue: c(0x7daea3),
            magenta: c(0xd3869b),
            cyan: c(0x89b482),
            white: c(0xddc7a1),
        }
    }

    pub fn pretty_in_pink() -> Self {
        Self {
            bg: c(0x1e1a1d),
            fg: c(0xffccec),
            black: c(0x1e1e1e),
            red: c(0xf6084c),
            green: c(0x67ff6d),
            yellow: c(0xffc44e),
            blue: c(0x2593be),
            magenta: c(0xd68bff),
            cyan: c(0x00fafa),
            white: c(0xe0def4),
        }
    }

    pub fn solarized_dark() -> Self {
        Self {
            bg: c(0x002b36),
            fg: c(0x839496),
            black: c(0x073642),
            red: c(0xdc322f),
            green: c(0x859900),
            yellow: c(0xb58900),
            blue: c(0x268bd2),
            magenta: c(0xd33682),
            cyan: c(0x2aa198),
            white: c(0xeee8d5),
        }
    }
}
