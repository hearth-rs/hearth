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

//! Defines the schema for interacting with glTF loading service.

use glam::Mat4;
use hearth_guest::LumpId;
use serde::{Deserialize, Serialize};

use crate::model::Model;

/// A request to the glTF loader service.
///
/// Responds with [GltfResponse].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum GltfRequest {
    /// Loads a binary or embedded glTF model.
    LoadSingle {
        /// The lump containing the glTF data.
        lump: LumpId,

        /// The root transform of the glTF.
        transform: Mat4,
    },
}

pub type GltfResponse = Result<Model, String>;
