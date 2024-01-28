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

use hearth_guest::debug_draw::*;

lazy_static::lazy_static! {
    static ref DEBUG_DRAW_FACTORY: RequestResponse<(), ()> =
        RequestResponse::expect_service("hearth.DebugDrawFactory");
}

/// An instance of debug draw.
pub struct DebugDraw {
    cap: Capability,
}

impl Drop for DebugDraw {
    fn drop(&mut self) {
        self.cap.send(&DebugDrawUpdate::Destroy, &[]);
    }
}

impl Default for DebugDraw {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugDraw {
    /// Creates a new debug draw mesh
    ///
    /// The contents of this mesh must be initialized with the update method
    pub fn new() -> Self {
        DebugDraw {
            cap: DEBUG_DRAW_FACTORY
                .request((), &[])
                .1
                .get(0)
                .unwrap()
                .clone(),
        }
    }

    /// Hide this debug draw mesh.
    pub fn hide(&self) {
        self.cap.send(&DebugDrawUpdate::Hide(true), &[]);
    }

    /// Show this debug draw mesh.
    pub fn show(&self) {
        self.cap.send(&DebugDrawUpdate::Hide(false), &[]);
    }

    /// Update the contents of this debug draw mesh.
    pub fn update(&self, mesh: DebugDrawMesh) {
        self.cap.send(&DebugDrawUpdate::Contents(mesh), &[]);
    }
}
