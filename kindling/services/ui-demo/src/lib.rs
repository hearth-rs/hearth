// Copyright (c) 2023 Roux
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

use glam::{Vec2, Vec3};
use hearth_guest::{canvas::*, window::*, *};
use raqote::*;

pub type CanvasFactory = RequestResponse<FactoryRequest, FactoryResponse>;

/// A wrapper around the canvas Capability.
struct CanvasWrapper {
    canvas: Capability,
}

impl CanvasWrapper {
    /// Send a new buffer of pixels to be drawn to the canvas.
    fn update(&self, buffer: Pixels) {
        self.canvas.send_json(&CanvasUpdate::Resize(buffer), &[]);
    }

    /// Send a new buffer to be drawn from a raqote DrawTarget.
    fn update_with_draw_target(&self, dt: DrawTarget) {
        self.update(Pixels {
            width: dt.width() as u32,
            height: dt.height() as u32,
            data: dt
                .get_data_u8()
                .chunks_exact(4)
                .map(|pix| [pix[2], pix[1], pix[0], pix[3]])
                .flatten()
                .collect(),
        });
    }
}

/// A UI slider object
struct Slider {
    track_size: Vec2,
    handle_pos: i32,
    handle_size: Vec2,
}

impl Default for Slider {
    fn default() -> Self {
        Self {
            track_size: Vec2::new(5.0, 200.0),
            handle_pos: 0,
            handle_size: Vec2::new(20.0, 5.0),
        }
    }
}

impl Slider {
    fn draw(&self, dt: &mut DrawTarget) {
        // draw the track of the slider
        dt.fill_rect(
            -(self.track_size.x / 2.0),
            0.0,
            self.track_size.x,
            self.track_size.y,
            &source_from_rgb(0xff, 0xff, 0xff),
            &DrawOptions::new(),
        );

        // draw the handle of the slider
        dt.fill_rect(
            -(self.handle_size.x / 2.0),
            0.0 + self.handle_pos as f32,
            self.handle_size.x,
            self.handle_size.y,
            &source_from_rgb(255, 0, 0),
            &DrawOptions::new(),
        );
    }
}

/// Helper function to initialize raqote Source from RGB values
fn source_from_rgb(r: u8, g: u8, b: u8) -> Source<'static> {
    Source::Solid(SolidSource::from_unpremultiplied_argb(255, r, g, b))
}

#[no_mangle]
pub extern "C" fn run() {
    let canvas_factory = CanvasFactory::new(
        REGISTRY
            .get_service("hearth.canvas.CanvasFactory")
            .expect("canvas factory service unavailable"),
    );

    let window = REGISTRY.get_service(SERVICE_NAME).unwrap();
    let events = Mailbox::new();
    let events_cap = events.make_capability(Permissions::SEND);
    window.send_json(&WindowCommand::Subscribe, &[&events_cap]);

    let canvas = spawn_canvas(&canvas_factory, CanvasSamplingMode::Nearest);
    let mut dt = DrawTarget::new(400, 400);
    dt.clear(SolidSource::from_unpremultiplied_argb(
        0xff, 0x15, 0x10, 0x14,
    ));
    let slider = Slider::default();
    dt.set_transform(&Transform::translation(100.0, 100.0));
    slider.draw(&mut dt);

    canvas.update_with_draw_target(dt);
}

/// Spawns a new canvas 1 unit in front of the camera's default position
fn spawn_canvas(canvas_factory: &CanvasFactory, sampling: CanvasSamplingMode) -> CanvasWrapper {
    let position = Position {
        origin: Vec3::new(0.0, 0.0, -1.0),
        orientation: Default::default(),
        half_size: Vec2::new(0.5, 0.5),
    };

    let request = FactoryRequest::CreateCanvas {
        position: position.clone(),
        pixels: Pixels {
            width: 1,
            height: 1,
            data: vec![0xff; 4],
        },
        sampling,
    };

    let (msg, caps) = canvas_factory.request(request, &[]);
    let _ = msg.unwrap();
    CanvasWrapper {
        canvas: caps
            .get(0)
            .expect("Canvas factory did not respond with Canvas capabulity")
            .to_owned(),
    }
}
