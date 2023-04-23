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

use std::ops::Deref;

use crate::error::{FontError, FontResult, GlyphShapeError};
use glam::Vec2;
use msdfgen::{Bitmap, FillRule, FontExt, MsdfGeneratorConfig, Range, Rgba};
use ttf_parser::{Face, GlyphId};

pub struct GlyphMtsdf {
    pub bitmap: GlyphBitmap,
    pub anchor: Vec2,
    pub px_per_em: f64,
}

impl Deref for GlyphMtsdf {
    type Target = GlyphBitmap;

    fn deref(&self) -> &GlyphBitmap {
        &self.bitmap
    }
}

impl GlyphMtsdf {
    pub fn generate(
        units_per_em: f64,
        px_per_em: f64,
        range: Range<f64>,
        angle_threshold: f64,
        face: &Face,
        glyph: GlyphId,
    ) -> FontResult<Self> {
        let config: MsdfGeneratorConfig = MsdfGeneratorConfig::default();
        let mut shape = face
            .glyph_shape(glyph)
            .ok_or(FontError::GlyphShape(GlyphShapeError(glyph)))?;
        shape.edge_coloring_simple(angle_threshold, 0);
        let bounds = shape.get_bound();
        let px_per_unit = px_per_em / units_per_em;
        let width = (bounds.width() * px_per_unit).ceil() as u32 + 8;
        let height = (bounds.height() * px_per_unit).ceil() as u32 + 8;
        let width = width.max(16);
        let height = height.max(16);
        let framing = bounds
            .autoframe(width, height, range, None)
            .ok_or(FontError::AutoFraming {
                glyph,
                width: width as usize,
                height: height as usize,
                range,
            })?;
        let mut bitmap = Bitmap::<Rgba<f32>>::new(width, height);
        shape.generate_mtsdf(&mut bitmap, framing, config);
        shape.correct_sign(&mut bitmap, framing, FillRule::default());
        shape.correct_msdf_error(&mut bitmap, framing, config);

        Ok(Self {
            bitmap: GlyphBitmap {
                data: bitmap
                    .pixels()
                    .iter()
                    .map(|p| {
                        fn conv(f: f32) -> u32 {
                            (f * 256.0).round() as u8 as _
                        }

                        (conv(p.r) << 24) | (conv(p.g) << 16) | (conv(p.b) << 8) | conv(p.a)
                    })
                    .collect(),
                width,
                height,
            },
            anchor: Vec2::new(framing.translate.x as f32, framing.translate.y as f32)
                / units_per_em as f32,
            px_per_em: px_per_em * framing.scale.x / px_per_unit,
        })
    }
}

pub struct GlyphBitmap {
    pub data: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

impl GlyphBitmap {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            data: vec![0; (width * height) as usize],
            width,
            height,
        }
    }

    pub fn data_bytes(&self) -> &[u8] {
        unsafe {
            let ptr = self.data.as_ptr();
            let ptr: *const u8 = std::mem::transmute(ptr);
            let len = self.data.len() * 4;
            std::slice::from_raw_parts(ptr, len)
        }
    }

    pub fn copy_to(&self, dst: &mut GlyphBitmap, x: u32, y: u32) {
        if self.width + x > dst.width || self.height + y > dst.height {
            panic!("copy_to out-of-bounds");
        }

        let mut src_cursor = 0;
        let mut dst_cursor = (y * dst.width + x) as usize;
        for _ in 0..self.height {
            let src_range = src_cursor..(src_cursor + self.width as usize);
            let dst_range = dst_cursor..(dst_cursor + self.width as usize);
            dst.data[dst_range].copy_from_slice(&self.data[src_range]);
            src_cursor += self.width as usize;
            dst_cursor += dst.width as usize;
        }
    }
}
