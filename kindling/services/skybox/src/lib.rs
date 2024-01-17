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

use hearth_guest::{renderer::*, Lump};
use kindling_host::prelude::{RequestResponse, REGISTRY};

type Renderer = RequestResponse<RendererRequest, RendererResponse>;

/// Helper function to append a skybox image to the cube texture data.
fn add_face(data: &mut Vec<u8>, image: &[u8]) {
    let decoded = image::load_from_memory(image).unwrap().into_rgba8();
    data.extend_from_slice(decoded.as_raw());
}

#[no_mangle]
pub extern "C" fn run() {
    let mut data = Vec::new();
    add_face(&mut data, include_bytes!("elyvisions/sh_ft.png"));
    add_face(&mut data, include_bytes!("elyvisions/sh_bk.png"));
    add_face(&mut data, include_bytes!("elyvisions/sh_up.png"));
    add_face(&mut data, include_bytes!("elyvisions/sh_dn.png"));
    add_face(&mut data, include_bytes!("elyvisions/sh_rt.png"));
    add_face(&mut data, include_bytes!("elyvisions/sh_lf.png"));

    let texture = Lump::load(
        &serde_json::to_vec(&TextureData {
            label: None,
            size: (1024, 1024).into(),
            data,
        })
        .unwrap(),
    )
    .get_id();

    let renderer = Renderer::new(REGISTRY.get_service("hearth.Renderer").unwrap());
    let (result, _) = renderer.request(RendererRequest::SetSkybox { texture }, &[]);
    result.unwrap();
}
