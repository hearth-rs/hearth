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

struct GridVertexOut {
    [[builtin(position)]] clip_position: vec4<f32>;
    [[location(0)]] uv: vec2<f32>;
};

struct GridUniform {
    mvp: mat4x4<f32>;
    size: vec2<f32>;
    tex_range: vec2<f32>;
    texture_size: vec4<f32>;
};

[[group(0), binding(0)]] var<uniform> grid: GridUniform;
[[group(0), binding(1)]] var t_grid: texture_2d<f32>;
[[group(0), binding(2)]] var s_grid: sampler;

[[stage(vertex)]]
fn grid_vs([[builtin(vertex_index)]] in_vertex_index: u32) -> GridVertexOut {
    let x = f32(i32(in_vertex_index & 1u));
    let y = f32(i32(in_vertex_index & 2u) / 2);
    let xy = vec2<f32>(x, y);

    let pos = (xy * 2.0 - 1.0) * grid.size;

    var out: GridVertexOut;
    out.clip_position = grid.mvp * vec4<f32>(pos, 0.0, 1.0);
    out.uv = xy * grid.tex_range;

    return out;
}

// this version of wgpu's WGSL doesn't support built-in smoothstep()
// we need to implement it ourselves
fn smoothstep(low: vec2<f32>, high: vec2<f32>, x: vec2<f32>) -> vec2<f32> {
    let t = clamp((x - low) / (high - low), vec2<f32>(0.0), vec2<f32>(1.0));
    return t * t * (3.0 - 2.0 * t);
}

[[stage(fragment)]]
fn grid_fs(frag: GridVertexOut) -> [[location(0)]] vec4<f32> {
    // the "pixel art upscaling" method comes from here:
    // https://www.youtube.com/watch?v=d6tp43wZqps

    // retrieve the texture size as a local variable
    let texture_size = grid.texture_size.xy;

    // box filter size in texel units
    let box_size = clamp(fwidth(frag.uv) * texture_size, vec2<f32>(1e-5), vec2<f32>(1.0));

    // scale uv by texture size to get texel coordinate
    let tx = frag.uv * texture_size - 0.5 * box_size;

    // compute offset for pixel-sized box filter
    let tx_offset = smoothstep(vec2<f32>(1.0) - box_size, vec2<f32>(1.0), fract(tx));

    // compute bilinear sample uv coordinates
    let uv = (floor(tx) + 0.5 + tx_offset) / texture_size;

    // sample the texture
    return textureSampleGrad(t_grid, s_grid, uv, dpdx(frag.uv), dpdy(frag.uv));
}
