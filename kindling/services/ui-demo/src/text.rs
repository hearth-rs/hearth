// Copyright (c) 2023 Marceline Cramer
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

use hearth_guest::canvas::Pixels;
use kindling_host::glam::{ivec2, IVec2};
use raqote::{DrawTarget, Transform};

use crate::{dt_to_pixels, source_from_rgb, DRAW_OPTIONS};

pub fn draw_text(font: &bdf::Font, content: &str) -> Pixels {
    // variables for calculating dimensions of drawn labels
    //
    // we initialize this based on the font's bounds to avoid weird
    // non-aligned layouts
    //
    // i32::MAX and i32::MIN are used so that all values will update the
    // min/max variables on the X axis
    let font_y = font.bounds().y;
    let font_height = font.bounds().height as i32;
    let mut bb_min = ivec2(i32::MAX, font_y);
    let mut bb_max = ivec2(i32::MIN, font_y + font_height);

    // laid-out glyphs
    let mut glyphs = Vec::new();

    // layout glyphs for every character
    let mut cursor = 0;
    for c in content.chars() {
        let Some(glyph) = font.glyphs().get(&c) else {
            continue;
        };

        // get offset of glyph bounding box
        let (mut ox, mut oy) = glyph
            .vector()
            .map(|(x, y)| (*x as i32, *y as i32))
            .unwrap_or((glyph.bounds().x, glyph.bounds().y));

        // adjust offset by cursor
        ox += cursor;

        // adjust Y-coordinate... somehow
        oy += glyph.bounds().height as i32 - font.bounds().height as i32 + 10;

        // calculate bounding box of glyph
        let tl = ivec2(ox, -oy);
        let br = tl + ivec2(glyph.width() as i32, -(glyph.height() as i32));

        // factor glyph's bb into run's bb
        bb_min = bb_min.min(tl);
        bb_max = bb_max.max(br);

        // push the glyph info to be drawn later
        glyphs.push((glyph, ox, oy));

        // step cursor
        cursor += glyph
            .device_width()
            .map(|w| w.0 as i32)
            .unwrap_or(glyph.width() as i32 + 1);
    }

    // create draw target
    let size = (bb_max - bb_min).max(IVec2::ZERO);
    let mut dt = DrawTarget::new(size.x, size.y);

    // set dt origin to bounding box's origin
    dt.set_transform(&Transform::translation(-bb_min.x as f32, -bb_min.y as f32));

    // draw glyphs to draw target
    for (glyph, ox, oy) in glyphs {
        for py in 0..glyph.height() {
            for px in 0..glyph.width() {
                if !glyph.get(px, py) {
                    continue;
                }

                dt.fill_rect(
                    (px as i32 + ox) as f32,
                    (py as i32 - oy) as f32,
                    1.0,
                    1.0,
                    &source_from_rgb(0, 0, 0),
                    &DRAW_OPTIONS,
                );
            }
        }
    }

    dt_to_pixels(&dt)
}
