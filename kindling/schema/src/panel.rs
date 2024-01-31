// Copyright (c) 2024 Marceline Cramer
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

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// A request to the panel manager.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PanelManagerRequest {
    /// Create a panel.
    ///
    /// Provide one capability to receive the panel's [PanelEvents][PanelEvent].
    CreatePanel {
        /// The panel's initial transform.
        transform: PanelTransform,

        /// If true, this panel cannot be interacted with from behind.
        one_sided: bool,
    },

    /// Enable and update the global cursor.
    UpdateCursor(Cursor),

    /// Disable the global cursor.
    DisableCursor,

    /// Redraw all panels.
    Redraw,
}

/// A cursor's current state.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Cursor {
    /// The cursor's origin.
    pub origin: Vec3,

    /// The cursor's direction.
    pub dir: Vec3,

    /// Whether the cursor's select button is clicked.
    pub select: bool,
}

/// An event occurring on a panel.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PanelEvent {
    /// Redraw this panel.
    ///
    /// Contains the time (in seconds) since the last redraw.
    Redraw(f32),

    /// The panel has moved to the provided transform.
    Move(PanelTransform),

    /// The panel has gained focus.
    FocusGained,

    /// The panel has lost focus.
    FocusLost,

    /// The panel has received a cursor event.
    CursorEvent {
        /// The position of the cursor on this panel.
        at: Vec2,

        /// The kind of event.
        kind: CursorEventKind,
    },
}

/// A panel's location information in space.
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct PanelTransform {
    /// The panel's center position.
    pub position: Vec3,

    /// The panel's orientation.
    pub orientation: Quat,

    /// The panel's half-size.
    pub half_size: Vec2,
}

/// A kind of cursor event. See [PanelEvent::CursorEvent].
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub enum CursorEventKind {
    /// The cursor has entered this panel.
    ///
    /// Carries the current state of the select button.
    Entered(bool),

    /// The cursor has left this panel.
    Left,

    /// The cursor has moved on the panel.
    Move,

    /// The cursor has pressed its select button.
    ClickDown,

    /// The cursor has released its select button.
    ClickUp,
}
