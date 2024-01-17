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

use crate::LumpId;
use serde::{Deserialize, Serialize};

/// A spawn message sent to the Wasm process spawner service.
///
/// The service replies with a message containing the decimal representation of
/// the new process's local process ID.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WasmSpawnInfo {
    /// The [LumpId] of the Wasm module lump source.
    pub lump: LumpId,

    /// The identifier of the entrypoint to execute. If not specified, runs
    /// the exported "run" function.
    pub entrypoint: Option<u32>,
}
