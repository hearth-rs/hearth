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

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use alacritty_terminal::term::cell::Flags;
use font_mud::glyph_atlas::GlyphAtlas;
use hearth_rend3::wgpu::{util::DeviceExt, *};
use owned_ttf_parser::{AsFaceRef, OwnedFace};

/// A kind of font used by a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontStyle {
    Regular,
    Italic,
    Bold,
    BoldItalic,
}

impl FontStyle {
    /// Convert from `alacritty_terminal`'s grid cell flags.
    pub fn from_cell_flags(flags: Flags) -> Self {
        if flags.contains(Flags::BOLD_ITALIC) {
            Self::BoldItalic
        } else if flags.contains(Flags::ITALIC) {
            Self::Italic
        } else if flags.contains(Flags::BOLD) {
            Self::Bold
        } else {
            Self::Regular
        }
    }
}

/// Generic container for all font faces used in a terminal. Eases
/// the writing of code manipulating all faces at once.
#[derive(Clone, Debug, Default)]
pub struct FontSet<T> {
    pub regular: T,
    pub italic: T,
    pub bold: T,
    pub bold_italic: T,
}

impl<T> FontSet<T> {
    pub fn map<O>(self, f: impl Fn(T) -> O) -> FontSet<O> {
        FontSet {
            regular: f(self.regular),
            italic: f(self.italic),
            bold: f(self.bold),
            bold_italic: f(self.bold_italic),
        }
    }

    pub fn for_each(self, mut f: impl FnMut(T)) {
        f(self.regular);
        f(self.italic);
        f(self.bold);
        f(self.bold_italic);
    }

    pub fn get(&self, style: FontStyle) -> &T {
        match style {
            FontStyle::Regular => &self.regular,
            FontStyle::Italic => &self.italic,
            FontStyle::Bold => &self.bold,
            FontStyle::BoldItalic => &self.bold_italic,
        }
    }

    pub fn get_mut(&mut self, style: FontStyle) -> &mut T {
        match style {
            FontStyle::Regular => &mut self.regular,
            FontStyle::Italic => &mut self.italic,
            FontStyle::Bold => &mut self.bold,
            FontStyle::BoldItalic => &mut self.bold_italic,
        }
    }

    pub fn zip<O>(self, other: FontSet<O>) -> FontSet<(T, O)> {
        FontSet {
            regular: (self.regular, other.regular),
            italic: (self.italic, other.italic),
            bold: (self.bold, other.bold),
            bold_italic: (self.bold_italic, other.bold_italic),
        }
    }

    pub fn as_ref(&self) -> FontSet<&T> {
        FontSet {
            regular: &self.regular,
            italic: &self.italic,
            bold: &self.bold,
            bold_italic: &self.bold_italic,
        }
    }

    pub fn as_mut(&mut self) -> FontSet<&mut T> {
        FontSet {
            regular: &mut self.regular,
            italic: &mut self.italic,
            bold: &mut self.bold,
            bold_italic: &mut self.bold_italic,
        }
    }
}

/// A font face and its MSDF glyph atlas.
pub struct FaceAtlas {
    pub face: OwnedFace,
    pub atlas: GlyphAtlas,
    pub texture: Texture,
    pub queue: Arc<Queue>,
    pub touched: Mutex<HashSet<u16>>,
}

impl FaceAtlas {
    /// Create a new atlas from a face. Note that this takes time to complete.
    pub fn new(face: OwnedFace, device: &Device, queue: Arc<Queue>) -> Self {
        let (atlas, _errors) = GlyphAtlas::new(face.as_face_ref()).unwrap();

        let size = Extent3d {
            width: atlas.width,
            height: atlas.height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture_with_data(
            &queue,
            &TextureDescriptor {
                label: Some("AlacrittyRoutine::glyph_texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            },
            &vec![0u8; (atlas.width * atlas.height * 4) as usize],
        );

        Self {
            face,
            atlas,
            texture,
            queue,
            touched: Default::default(),
        }
    }

    /// Generate and upload a glyph bitmap for each glyph that hasn't already been.
    pub fn touch(&self, glyphs: &[u16]) {
        let mut touched = self.touched.lock().unwrap();
        for glyph in glyphs {
            if touched.insert(*glyph) {
                let glyph = self.atlas.glyphs.get(*glyph as usize);
                let Some(Some(glyph)) = glyph else { continue };
                let bitmap = glyph.shape.generate();

                self.queue.write_texture(
                    ImageCopyTexture {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: Origin3d {
                            x: glyph.position.x,
                            y: glyph.position.y,
                            z: 0,
                        },
                        aspect: TextureAspect::All,
                    },
                    bitmap.data_bytes(),
                    ImageDataLayout {
                        offset: 0,
                        bytes_per_row: std::num::NonZeroU32::new(glyph.size.x * 4),
                        rows_per_image: std::num::NonZeroU32::new(glyph.size.y),
                    },
                    Extent3d {
                        width: glyph.size.x,
                        height: glyph.size.y,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
}
