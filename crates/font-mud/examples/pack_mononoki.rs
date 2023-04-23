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

use font_mud::glyph_atlas::GlyphAtlas;
use std::fs::File;

fn main() {
    let ttf_src = include_bytes!("../../../resources/mononoki/mononoki-Regular.ttf");
    let face = ttf_parser::Face::parse(ttf_src, 0).unwrap();
    let (glyph_atlas, _glyph_shape_errors) = GlyphAtlas::new(&face).unwrap();
    let bitmap = &glyph_atlas.bitmap;
    let output = File::create("mononoki.png").unwrap();
    let mut encoder = png::Encoder::new(output, bitmap.width, bitmap.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(bitmap.data_bytes()).unwrap();
}
