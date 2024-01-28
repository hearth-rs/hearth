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

use super::*;

use hearth_guest::terminal::*;

lazy_static::lazy_static! {
    static ref TERMINAL_FACTORY: RequestResponse<FactoryRequest, FactoryResponse> =
        RequestResponse::expect_service("hearth.terminal.TerminalFactory");
}

/// A wrapper around the Terminal Capability.
pub struct Terminal {
    cap: Capability,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.cap.send(&TerminalUpdate::Quit, &[]);
    }
}

impl Terminal {
    /// Creates a new terminal with the given TerminalState.
    ///
    /// Panics if the factory responds with an error.
    pub fn new(state: TerminalState) -> Self {
        let resp = TERMINAL_FACTORY.request(FactoryRequest::CreateTerminal(state), &[]);
        let _ = resp.0.unwrap();
        Terminal {
            cap: resp.1.get(0).unwrap().clone(),
        }
    }

    /// Send input to this terminal.
    pub fn input(&self, input: String) {
        self.cap.send(&TerminalUpdate::Input(input), &[])
    }

    /// Update the state of this terminal.
    pub fn update(&self, state: TerminalState) {
        self.cap.send(&TerminalUpdate::State(state), &[])
    }
}
