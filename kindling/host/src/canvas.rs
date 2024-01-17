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

use super::*;

use hearth_guest::canvas::*;

lazy_static::lazy_static! {
    /// A lazily-initialized handle to the canvas factory service.
    static ref CANVAS_FACTORY: RequestResponse<FactoryRequest, FactoryResponse> = {
        RequestResponse::new(registry::REGISTRY.get_service("hearth.canvas.CanvasFactory").unwrap())
    };
}

/// A wrapper around the canvas Capability.
pub struct Canvas {
    cap: Capability,
}

impl Canvas {
    /// Creates a new Canvas.
    ///
    /// Panics if the factory responds with an error.
    pub fn new(position: Position, pixels: Pixels, sampling: CanvasSamplingMode) -> Self {
        let resp = CANVAS_FACTORY.request(
            FactoryRequest::CreateCanvas {
                position,
                pixels,
                sampling,
            },
            &[],
        );
        let _ = resp.0.unwrap();
        Canvas {
            cap: resp.1.get(0).unwrap().clone(),
        }
    }

    /// Update this canvas with a new buffer of pixels to draw.
    pub fn update(&self, buffer: Pixels) {
        self.cap.send_json(&CanvasUpdate::Resize(buffer), &[]);
    }

    /// Move this canvas to a new position in 3D space.
    pub fn relocate(&self, position: Position) {
        self.cap.send_json(&CanvasUpdate::Relocate(position), &[])
    }

    /// Blit a recatangular buffer to a part of this canvas.
    pub fn blit(&self, blit: Blit) {
        self.cap.send_json(&CanvasUpdate::Blit(blit), &[])
    }
}
