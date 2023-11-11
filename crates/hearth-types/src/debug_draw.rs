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

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DebugDrawVertex {
    /// The position of this vertex in world space.
    pub position: Vec3,

    /// The color of this vertex. Alpha is ignored and fixed to opaque.
    pub color: Color,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DebugDrawMesh {
    pub vertices: Vec<DebugDrawVertex>,
    pub indices: Vec<u32>,
}

/// An update to a debug draw mesh.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum DebugDrawUpdate {
    /// Updates the contents of this debug draw mesh.
    Contents(DebugDrawMesh),

    /// Sets whether to hide this mesh.
    Hide(bool),

    /// Destroys this debug draw mesh.
    Destroy,
}
