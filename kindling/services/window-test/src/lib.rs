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

use hearth_guest::window::WindowEvent;
use kindling_host::prelude::*;

hearth_guest::export_metadata!();

#[no_mangle]
pub extern "C" fn run() {
    let events = MAIN_WINDOW.subscribe();

    loop {
        let (msg, _) = events.recv::<WindowEvent>();

        if let WindowEvent::Redraw { .. } = msg {
            continue;
        }

        info!(?msg, "window event");
    }
}
