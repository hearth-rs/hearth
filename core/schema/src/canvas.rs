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

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

/// A rectangular buffer of pixel data.
#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pixels {
    /// The width of the buffer, in pixels.
    pub width: u32,

    /// The height of the buffer, in pixels.
    pub height: u32,

    /// The RGBA color data of the buffer.
    ///
    /// `width * height * 4` should match the length of `data`. Missing pixel
    /// data will be initialized with `0xff` for all components. Excess data
    /// is ignored.
    #[serde_as(as = "Base64")]
    pub data: Vec<u8>,
}

/// A rectangular update to a target region of a canvas's pixel buffer.
///
/// Out-of-bounds regions of blits are discarded.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Blit {
    /// The X coordinate of this blit's origin in pixels.
    pub x: u32,

    /// The Y coordinate of this blit's origin in pixels.
    pub y: u32,

    /// The pixels to copy to this blit's position.
    pub pixels: Pixels,
}

/// The positioning of a canvas in 3D space.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Position {
    /// The origin of this canvas.
    pub origin: Vec3,

    /// The orientation (aka rotation) of this canvas.
    pub orientation: Quat,

    /// The half-size (distance from the center to the edge) of this canvas.
    ///
    /// Unrelated to the canvas's pixel size. The canvas will stretch its pixel
    /// buffer to fit the half-size.
    pub half_size: Vec2,
}

/// A message to update a canvas instance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum CanvasUpdate {
    /// Relocate the canvas to a given [Position].
    Relocate(Position),

    /// Resize the canvas using [Pixels].
    ///
    /// The canvas's new pixel buffer size is derived from the pixel buffer's size.
    ///
    /// If the given pixel buffer's size is the same as the current canvas's
    /// size, the canvas will be efficiently updated without reallocating any
    /// GPU memory.
    Resize(Pixels),

    /// Blit a buffer to a part of this canvas.
    Blit(Blit),
}

/// Configures the method of texture sampling to use for a canvas.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub enum CanvasSamplingMode {
    /// Uses bilinear texture sampling.
    Linear,

    /// Uses anti-aliased nearest-neighbor texture sampling.
    Nearest,
}

/// A request to the canvas factory.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FactoryRequest {
    /// Create a new canvas.
    ///
    /// Returns a capabiliity via [FactorySuccess::Canvas] to a canvas instance,
    /// which receives [CanvasUpdate] messages.
    CreateCanvas {
        /// The canvas's initial position.
        position: Position,

        /// The initial contents of the canvas's pixel buffer.
        pixels: Pixels,

        /// The sampling method to use.
        sampling: CanvasSamplingMode,
    },
}

/// A success response from a [FactoryRequest].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FactorySuccess {
    /// A canvas was successfully created.
    Canvas,
}

/// An error response from a [FactoryRequest].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FactoryError {
    /// The request has failed to parse.
    ParseError,
}

/// A type shorthand for [FactorySuccess] and [FactoryError].
pub type FactoryResponse = Result<FactorySuccess, FactoryError>;
