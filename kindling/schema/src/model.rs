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

//! A general schema for 3D models.

use glam::Mat4;
use hearth_guest::LumpId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Model {
    /// The set of [Meshes][Mesh] composing this model.
    pub meshes: Vec<Mesh>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Mesh {
    /// This mesh's transform.
    pub transform: Mat4,

    /// The [LumpId] of this mesh's `MaterialData`, as defined by the host
    /// renderer schema.
    pub material: LumpId,

    /// The [LumpId] of this mesh's `MeshDat`, as defined by the host
    /// renderer schema.
    pub mesh: LumpId,
}
