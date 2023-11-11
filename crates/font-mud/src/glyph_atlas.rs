// Copyright (c) 2023 the Hearth contributors.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use glam::{uvec2, UVec2, Vec2};
use msdfgen::Range;
use rect_packer::Packer;
use ttf_parser::{Face, GlyphId};

use crate::{
    error::{FontError, FontResult, GlyphShapeError},
    glyph_bitmap::{GlyphBitmap, GlyphShape},
};

#[derive(Copy, Clone, Debug)]
pub struct GlyphVertex {
    pub position: Vec2,
    pub tex_coords: Vec2,
}

pub struct GlyphInfo {
    pub position: UVec2,
    pub size: UVec2,
    pub anchor: Vec2,
    pub shape: GlyphShape,
    pub vertices: [GlyphVertex; 4],
}

pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub glyphs: Vec<Option<GlyphInfo>>,
}

impl GlyphAtlas {
    pub const PX_PER_EM: f64 = 24.0;
    pub const RANGE: Range<f64> = Range::Px(8.0);
    pub const ANGLE_THRESHOLD: f64 = 3.0;

    /// turns a face into a glyph atlas.
    /// all fonts have some glyph shape errors for some reason, we pass those through, as we treat them as non-fatal errors.
    pub fn new(face: &Face) -> FontResult<(GlyphAtlas, Vec<GlyphShapeError>)> {
        let mut glyphs = Vec::with_capacity(face.number_of_glyphs() as usize);
        let mut glyph_shape_errors = vec![];
        for c in 0..face.number_of_glyphs() {
            let glyph = GlyphShape::new(
                face.units_per_em() as f64,
                Self::PX_PER_EM,
                Self::RANGE,
                Self::ANGLE_THRESHOLD,
                face,
                GlyphId(c),
            );

            match glyph {
                Ok(glyph) => {
                    glyphs.push(Some(glyph));
                }
                Err(err) => {
                    match err {
                        FontError::GlyphShape(glyph_shape_error) => {
                            glyph_shape_errors.push(glyph_shape_error);
                        }
                        error => return Err(error),
                    }
                    glyphs.push(None);
                }
            }
        }

        let (atlas_size, packed) = Self::pack(&glyphs);
        let texture_size = atlas_size.as_vec2();

        let glyphs: Vec<_> = packed
            .into_iter()
            .zip(glyphs)
            .map(|glyph| {
                if let (Some(position), Some(glyph)) = glyph {
                    let scale = (1.0 / glyph.px_per_em) as f32;
                    let offset = glyph.anchor - 0.5 * scale;

                    let tex_offset = position.as_vec2() / texture_size;
                    let size = Vec2::new(glyph.width as f32, glyph.height as f32) - 1.0;
                    let v1 = Vec2::ZERO;
                    let v2 = Vec2::new(size.x, 0.0);
                    let v3 = Vec2::new(0.0, size.y);
                    let v4 = size;

                    Some(GlyphInfo {
                        position,
                        size: UVec2::new(glyph.width, glyph.height),
                        anchor: offset,
                        shape: glyph,
                        vertices: [
                            GlyphVertex {
                                position: v1 * scale - offset,
                                tex_coords: v1 / texture_size + tex_offset,
                            },
                            GlyphVertex {
                                position: v2 * scale - offset,
                                tex_coords: v2 / texture_size + tex_offset,
                            },
                            GlyphVertex {
                                position: v3 * scale - offset,
                                tex_coords: v3 / texture_size + tex_offset,
                            },
                            GlyphVertex {
                                position: v4 * scale - offset,
                                tex_coords: v4 / texture_size + tex_offset,
                            },
                        ],
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok((
            GlyphAtlas {
                width: atlas_size.x,
                height: atlas_size.y,
                glyphs,
            },
            glyph_shape_errors,
        ))
    }

    fn pack(glyphs: &Vec<Option<GlyphShape>>) -> (UVec2, Vec<Option<UVec2>>) {
        let mut config = rect_packer::Config {
            width: 256,
            height: 256,
            border_padding: 0,
            rectangle_padding: 0,
        };

        let mut packer = Packer::new(config);
        let mut last_switched_width = false;

        let packed = loop {
            let mut out_of_room = false;
            let mut packed = Vec::with_capacity(glyphs.len());

            for glyph in glyphs.iter() {
                let Some(glyph) = glyph else {
                    packed.push(None);
                    continue;
                };

                let Some(rect) = packer.pack(glyph.width as i32, glyph.height as i32, false) else {
                    out_of_room = true;
                    break;
                };

                let position = uvec2(rect.x as u32, rect.y as u32);
                packed.push(Some(position));
            }

            if out_of_room {
                if last_switched_width {
                    config.height *= 2;
                } else {
                    config.width *= 2;
                }

                last_switched_width = !last_switched_width;
                packer = Packer::new(config);
            } else {
                break packed;
            }
        };

        (uvec2(config.width as u32, config.height as u32), packed)
    }

    pub fn generate_full(&self) -> GlyphBitmap {
        let mut bitmap = GlyphBitmap::new(self.width, self.height);

        for glyph in self.glyphs.iter().flatten() {
            let glyph_bitmap = glyph.shape.generate();
            glyph_bitmap.copy_to(&mut bitmap, glyph.position.x, glyph.position.y);
        }

        bitmap
    }
}
