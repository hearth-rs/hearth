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

use glam::{vec2, Vec2, Vec3};
use hearth_guest::{canvas::*, window::*, *};
use raqote::*;

pub type CanvasFactory = RequestResponse<FactoryRequest, FactoryResponse>;

static DRAW_OPTIONS: DrawOptions = DrawOptions {
    antialias: AntialiasMode::None,
    blend_mode: BlendMode::SrcOver,
    alpha: 1.,
};

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
    fn update_with_draw_target(&self, dt: &DrawTarget) {
        self.update(Pixels {
            width: dt.width() as u32,
            height: dt.height() as u32,
            data: dt
                .get_data_u8()
                .chunks_exact(4)
                .flat_map(|pix| [pix[2], pix[1], pix[0], pix[3]])
                .collect(),
        });
    }
}

/// A UI slider object
struct Slider<'a> {
    track_size: Vec2,
    track_source: Source<'a>,
    handle_pos: i32,
    handle_grab: Option<i32>,
    handle_size: Vec2,
    handle_source: Source<'a>,
    handle_grab_source: Source<'a>,
}

impl<'a> Default for Slider<'a> {
    fn default() -> Self {
        Self {
            track_size: Vec2::new(4.0, 100.0),
            track_source: source_from_rgb(255, 255, 255),
            handle_pos: 1,
            handle_grab: None,
            handle_size: Vec2::new(20.0, 8.0),
            handle_source: source_from_rgb(255, 255, 255),
            handle_grab_source: source_from_rgb(0xd7, 0xd9, 0xd6),
        }
    }
}

impl<'a> Slider<'a> {
    fn draw(&self, dt: &mut DrawTarget) {
        let half_handle_size = self.handle_size * 0.5;
        let half_track_size = self.track_size * 0.5;
        let mut pb = PathBuilder::new();
        pb.rect(
            -half_track_size.x,
            0.0,
            self.track_size.x,
            self.track_size.y,
        );
        let path = pb.finish();
        dt.fill(&path, &self.track_source, &DRAW_OPTIONS);
        dt.stroke(
            &path,
            &source_from_rgb(0, 0, 0),
            &StrokeStyle {
                width: 1.0,
                join: LineJoin::Round,
                ..Default::default()
            },
            &DRAW_OPTIONS,
        );

        let mut draw_tick = |pos: Vec2| {
            let mut pb = PathBuilder::new();
            pb.rect(pos.x, pos.y - 1.0, 5.0, 2.0);
            let path = pb.finish();
            dt.fill(&path, &source_from_rgb(255, 255, 255), &DRAW_OPTIONS);
            dt.stroke(
                &path,
                &source_from_rgb(0, 0, 0),
                &StrokeStyle {
                    width: 1.0,
                    join: LineJoin::Round,
                    ..Default::default()
                },
                &DRAW_OPTIONS,
            );
        };
        let l_tick = -10.0;
        let r_tick = 5.0;
        draw_tick(vec2(l_tick, 1.0));
        draw_tick(vec2(r_tick, 1.0));
        draw_tick(vec2(l_tick, half_track_size.y));
        draw_tick(vec2(r_tick, half_track_size.y));
        draw_tick(vec2(l_tick, self.track_size.y - 1.0));
        draw_tick(vec2(r_tick, self.track_size.y - 1.0));

        let handle_source = if self.handle_grab.is_some() {
            self.handle_grab_source.clone()
        } else {
            self.handle_source.clone()
        };

        // draw the handle of the slider
        let mut pb = PathBuilder::new();
        pb.rect(
            -(half_handle_size.x),
            -(half_handle_size.y) + self.handle_pos as f32,
            self.handle_size.x,
            self.handle_size.y,
        );
        let path = pb.finish();
        dt.fill(&path, &handle_source, &DRAW_OPTIONS);
        dt.stroke(
            &path,
            &source_from_rgb(0, 0, 0),
            &StrokeStyle {
                width: 1.0,
                join: LineJoin::Round,
                ..Default::default()
            },
            &DRAW_OPTIONS,
        );
    }

    fn on_drag_start(&mut self, pos: Vec2) {
        let handle_pos = Vec2::new(0.0, self.handle_pos as f32);
        let pos = (pos - handle_pos).abs() * 2.0;

        if pos.x > self.handle_size.x || pos.y > self.handle_size.y {
            return;
        }

        self.handle_grab = Some(self.handle_pos);
    }

    fn on_drag_end(&mut self) {
        self.handle_grab = None;
    }

    fn on_drag_move(&mut self, delta: Vec2) {
        if let Some(grab) = self.handle_grab.as_ref() {
            self.handle_pos = *grab + delta.y.round() as i32;
            self.handle_pos = self
                .handle_pos
                .clamp(1, self.track_size.y.ceil() as i32 - 1);
        }
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

    window.send_json(
        &WindowCommand::SetCamera {
            vfov: 90.0,
            near: 0.01,
            view: Default::default(),
        },
        &[],
    );

    let canvas = spawn_canvas(&canvas_factory, CanvasSamplingMode::Nearest);
    let canvas_size = (400, 400);
    let mut dt = DrawTarget::new(canvas_size.0, canvas_size.1);
    let mut window_size = Vec2::new(10.0, 10.0);
    let mut slider = Slider::default();
    let mut cursor_pos = Vec2::ZERO;
    let slider_pos = Vec2::new(100.0, 100.0);
    let mut grab_start: Option<Vec2> = None;

    loop {
        let mut redraw = false;

        loop {
            let (msg, _) = events.recv_json::<WindowEvent>();

            match msg {
                WindowEvent::Resized(size) => {
                    window_size = size.as_vec2();
                }
                WindowEvent::CursorMoved { position: new_pos } => {
                    let aspect = window_size.x / window_size.y;
                    let window_space = new_pos.as_vec2() / window_size;

                    let x = window_space.x * aspect - (aspect - 1.0) / 2.0;
                    let y = window_space.y;

                    cursor_pos = (Vec2::new(x, y)
                        * Vec2::new(canvas_size.0 as f32, canvas_size.1 as f32))
                    .round();

                    if let Some(start) = grab_start.as_ref() {
                        let delta = cursor_pos - *start;
                        slider.on_drag_move(delta);
                        redraw = true;
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                } => {
                    grab_start = Some(cursor_pos);
                    slider.on_drag_start(cursor_pos - slider_pos);
                    redraw = true;
                }
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                } => {
                    grab_start = None;
                    slider.on_drag_end();
                    redraw = true;
                }
                WindowEvent::Redraw { .. } => {
                    break;
                }
                _ => {}
            }
        }

        if !redraw {
            continue;
        }

        dt.clear(SolidSource::from_unpremultiplied_argb(
            0xff, 0xd6, 0xf4, 0xfe,
        ));

        dt.set_transform(&Transform::translation(slider_pos.x, slider_pos.y));
        slider.draw(&mut dt);
        canvas.update_with_draw_target(&dt);
    }
}

/// Spawns a new canvas 1 unit in front of the camera's default position
fn spawn_canvas(canvas_factory: &CanvasFactory, sampling: CanvasSamplingMode) -> CanvasWrapper {
    let position = Position {
        origin: Vec3::new(0.0, 0.0, -1.0),
        orientation: Default::default(),
        half_size: Vec2::new(1.0, 1.0),
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
