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

use super::{glam::Mat4, *};

use hearth_guest::window::*;

lazy_static::lazy_static! {
    /// The main client window.
    pub static ref MAIN_WINDOW: Window =  {
        Window {
            cap: registry::REGISTRY.get_service(SERVICE_NAME).unwrap()
        }
    };
}

/// Instance of a desktop window.
pub struct Window {
    cap: Capability,
}

impl Window {
    /// Subscribe to the window events published by this window.
    ///
    /// Returns a Mailbox that recieves all window events.
    pub fn subscribe(&self) -> Mailbox {
        let mailbox = Mailbox::new();
        let reply_cap = mailbox.make_capability(Permissions::SEND | Permissions::MONITOR);
        self.cap.send_json(&WindowCommand::Subscribe, &[&reply_cap]);
        mailbox
    }

    /// Sets the title of this window.
    pub fn set_title(&self, title: String) {
        self.cap.send_json(&WindowCommand::SetTitle(title), &[]);
    }

    /// Set the cursor's grab mode.
    pub fn cursor_grab_mode(&self, mode: CursorGrabMode) {
        self.cap.send_json(&WindowCommand::SetCursorGrab(mode), &[]);
    }

    /// Shows the window's cursor.
    pub fn show_cursor(&self) {
        self.cap
            .send_json(&WindowCommand::SetCursorVisible(true), &[]);
    }

    /// Hide the window's cursor.
    pub fn hide_cursor(&self) {
        self.cap
            .send_json(&WindowCommand::SetCursorVisible(false), &[]);
    }

    /// Update the window's rending camera
    ///
    /// `vfov` - The vertical field of view, in degrees.
    /// `near` - Near plane distance. All projection uses an infinite far plan.
    /// `view` - The camera's view matrix.
    pub fn set_camera(&self, vfov: f32, near: f32, view: Mat4) {
        self.cap
            .send_json(&WindowCommand::SetCamera { vfov, near, view }, &[]);
    }
}
