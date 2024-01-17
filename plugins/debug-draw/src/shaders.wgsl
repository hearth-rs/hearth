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

struct VertexIn {
    [[location(0)]] position: vec3<f32>;
    [[location(1)]] color: vec4<f32>;
};

struct VertexOut {
    [[builtin(position)]] clip_position: vec4<f32>;
    [[location(0)]] color: vec4<f32>;
};

struct CameraUniform {
    mvp: mat4x4<f32>;
};

[[group(0), binding(0)]] var<uniform> camera: CameraUniform;

fn srgb_to_linear(l: vec3<f32>) -> vec3<f32> {
    let cutoff = l > vec3<f32>(0.0405);
    let lower = l / vec3<f32>(12.92);
    let higher = pow((l + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(lower, higher, cutoff);
}

[[stage(vertex)]]
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = camera.mvp * vec4<f32>(in.position, 1.0);
    out.color = vec4<f32>(srgb_to_linear(in.color.bgr), 1.0);
    return out;
}

[[stage(fragment)]]
fn fs_main(frag: VertexOut) -> [[location(0)]] vec4<f32> {
    return frag.color;
}
