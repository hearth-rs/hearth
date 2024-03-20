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

use std::f32::consts::PI;

use hearth_guest::{renderer::DirectionalLightState, Lump, PARENT};
use kindling_host::{
    glam::{vec3, Mat4, Vec3},
    prelude::*,
    renderer::{set_ambient_lighting, DirectionalLight, Object, ObjectConfig},
};
use kindling_schema::gltf::{GltfRequest, GltfResponse};

hearth_guest::export_metadata!();

#[no_mangle]
pub extern "C" fn run() {
    set_ambient_lighting(Vec3::new(0.1, 0.1, 0.1));

    let _light = DirectionalLight::new(DirectionalLightState {
        color: Vec3::ONE,
        intensity: 10.0,
        direction: Vec3::new(0.1, -1.0, 0.1).normalize(),
        distance: 10.0,
    });

    spawn_loader(
        include_bytes!("WaterBottle.glb"),
        Mat4::from_translation(vec3(0.0, -1.0, 0.0)),
    );

    spawn_loader(
        include_bytes!("DamagedHelmet.glb"),
        Mat4::from_translation(vec3(2.0, -1.0, 1.7)) * Mat4::from_rotation_y(PI / 2.0),
    );

    spawn_loader(
        include_bytes!("korakoe.vrm"),
        Mat4::from_translation(vec3(-2.0, -1.0, 1.7)) * Mat4::from_rotation_y(PI / -2.0),
    );
}

fn spawn_loader(src: &[u8], transform: Mat4) {
    let lump = Lump::load_raw(src).get_id();
    let child = spawn_fn(loader, None);
    child.send(&(lump, transform), &[]);
}

fn loader() {
    let ((lump, transform), _caps) = PARENT.recv();

    let gltf =
        RequestResponse::<GltfRequest, GltfResponse>::expect_service("rs.hearth.kindling.glTF");

    let (result, _caps) = gltf.request(GltfRequest::LoadSingle { lump, transform }, &[]);

    let model = result.unwrap();

    for object in model.meshes {
        let mesh = Lump::load_by_id(&object.mesh);
        let material = Lump::load_by_id(&object.material);

        let object = Object::new(ObjectConfig {
            mesh: &mesh,
            skeleton: None,
            material: &material,
            transform,
        });

        // don't destroy the mesh
        std::mem::forget(object);
    }
}
