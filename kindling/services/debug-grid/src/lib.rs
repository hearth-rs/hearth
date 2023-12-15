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

use hearth_guest::{debug_draw::*, Color};
use kindling_host::prelude::{glam::vec3, DebugDraw};

#[no_mangle]
pub extern "C" fn run() {
    let size = 15;
    let color = Color::from_rgb(0x6a, 0xf5, 0xfc);
    let grid_to_pos = |x: i32, y: i32| vec3(x as f32 * 5.0, -8.0, y as f32 * 5.0);
    let vertex = |x, y, color| -> DebugDrawVertex {
        DebugDrawVertex {
            position: grid_to_pos(x, y),
            color,
        }
    };

    let mut vertices = Vec::new();

    for x in -size..=size {
        vertices.push(vertex(x, -size, color));
        vertices.push(vertex(x, size, color));
    }

    for y in -size..=size {
        vertices.push(vertex(-size, y, color));
        vertices.push(vertex(size, y, color));
    }

    let dd = DebugDraw::new();
    dd.update(DebugDrawMesh {
        indices: (0..vertices.len() as u32).collect(),
        vertices,
    });
    std::mem::forget(dd);
}
