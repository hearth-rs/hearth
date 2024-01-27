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

use hearth_guest::{wasm::*, LumpId};

lazy_static::lazy_static! {
    static ref WASM_SPAWNER: RequestResponse<wasm::WasmSpawnInfo, ()> =
        RequestResponse::expect_service("hearth.wasm.WasmProcessSpawner");
}

/// Spawns a child process for the given function.
///
/// Takes an optional capability to a registry. If provided, the service will
/// be added to the given registry, otherwise it will be added to the default
/// registry.
pub fn spawn_fn(cb: fn(), registry: Option<Capability>) -> Capability {
    // directly transmute a Rust function pointer to a Wasm function index
    let entrypoint = cb as usize as u32;

    let ((), caps) = WASM_SPAWNER.request(
        wasm::WasmSpawnInfo {
            lump: hearth_guest::this_lump(),
            entrypoint: Some(entrypoint),
        },
        &[registry.as_ref().unwrap_or(registry::REGISTRY.as_ref())],
    );

    caps.get(0).cloned().unwrap()
}

/// Spawn an entire Wasm module from a given lump.
///
/// Takes an optional capability to a registry. If provided, the service will
/// be added to the given registry, otherwise it will be added to the default
/// registry.
pub fn spawn_mod(lump: LumpId, registry: Option<Capability>) -> Capability {
    let ((), caps) = WASM_SPAWNER.request(
        wasm::WasmSpawnInfo {
            lump,
            entrypoint: None,
        },
        &[registry.as_ref().unwrap_or(registry::REGISTRY.as_ref())],
    );
    caps.get(0).cloned().unwrap()
}
