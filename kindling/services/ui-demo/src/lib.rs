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

use std::sync::Arc;

use hearth_guest::{canvas::*, window::*};
use kindling_host::{
    glam::{ivec2, uvec2, IVec2, UVec2},
    prelude::{
        glam::{vec2, vec3, Vec2},
        *,
    },
};
use raqote::*;

static DRAW_OPTIONS: DrawOptions = DrawOptions {
    antialias: AntialiasMode::None,
    blend_mode: BlendMode::SrcOver,
    alpha: 1.,
};

fn dt_to_pixels(dt: &DrawTarget) -> Pixels {
    Pixels {
        width: dt.width() as u32,
        height: dt.height() as u32,
        data: dt
            .get_data_u8()
            .chunks_exact(4)
            .flat_map(|pix| [pix[2], pix[1], pix[0], pix[3]])
            .collect(),
    }
}

/// A set of constraints for the size of a widget.
///
/// See https://docs.flutter.dev/ui/layout/constraints for more info.
pub struct Constraints {
    /// The minimum available dimensions of the widget.
    pub min: UVec2,

    /// The maximum available dimensions of the widget.
    pub max: UVec2,
}

/// An input event.
#[derive(Copy, Clone, Debug)]
pub enum InputEvent {
    DragStart(IVec2),
    DragEnd,
    DragMove(IVec2),
}

impl InputEvent {
    pub fn offset(self, offset: IVec2) -> Self {
        use InputEvent::*;
        match self {
            DragStart(pos) => DragStart(pos + offset),
            event => event,
        }
    }
}

pub trait Widget: 'static {
    /// Propagates layout constraints to this widget and returns the new widget
    /// size.
    fn layout(&mut self, constraints: &Constraints) -> UVec2;

    /// Draws this widget.
    fn draw(&self) -> Pixels;

    /// Processes an incoming input event.
    fn on_input(&mut self, event: InputEvent);
}

/// A helper struct for managing [Widget] implementations in other widgets.
///
/// Caches data for reuse, such as the last rendered pixel buffer.
pub struct Child {
    /// The inner [Widget] implementation.
    inner: Box<dyn Widget>,

    /// The current position of this widget.
    position: UVec2,
}

impl Child {
    /// Initializes a child with position at (0, 0).
    pub fn new(inner: impl Widget) -> Self {
        Self {
            inner: Box::new(inner),
            position: UVec2::ZERO,
        }
    }

    /// Draws this child within the draw target at this child's position.
    pub fn draw(&self, dt: &mut DrawTarget) {
        let pixels = self.inner.draw();

        let image = Image {
            width: pixels.width as i32,
            height: pixels.height as i32,
            data: unsafe { std::mem::transmute(pixels.data.as_slice()) },
        };

        dt.draw_image_at(
            self.position.x as f32,
            self.position.y as f32,
            &image,
            &DRAW_OPTIONS,
        );
    }
}

pub enum FlowDirection {
    Horizontal,
    Vertical,
}

pub struct Flow {
    dir: FlowDirection,
    size: UVec2,
    children: Vec<Child>,
}

impl Widget for Flow {
    fn layout(&mut self, constraints: &Constraints) -> UVec2 {
        let child_constraints = match self.dir {
            FlowDirection::Horizontal => Constraints {
                min: uvec2(0, constraints.min.y),
                max: uvec2(u32::MAX, constraints.max.y),
            },
            FlowDirection::Vertical => Constraints {
                min: uvec2(constraints.min.x, 0),
                max: uvec2(constraints.max.x, u32::MAX),
            },
        };

        let mut cursor = 0;
        let mut max_cross_size = 0;

        for child in self.children.iter_mut() {
            let size = child.inner.layout(&child_constraints);

            let (cursor_step, main_size, cross_size) = match self.dir {
                FlowDirection::Horizontal => (uvec2(cursor, 0), size.x, size.y),
                FlowDirection::Vertical => (uvec2(0, cursor), size.y, size.x),
            };

            child.position += cursor_step;
            cursor += main_size;
            max_cross_size = cross_size.max(max_cross_size);
        }

        self.size = match self.dir {
            FlowDirection::Horizontal => uvec2(cursor, max_cross_size),
            FlowDirection::Vertical => uvec2(max_cross_size, cursor),
        };

        self.size
    }

    fn draw(&self) -> Pixels {
        let mut dt = DrawTarget::new(self.size.x as i32, self.size.y as i32);

        for child in self.children.iter() {
            child.draw(&mut dt);
        }

        dt_to_pixels(&dt)
    }

    fn on_input(&mut self, event: InputEvent) {
        for child in self.children.iter_mut() {
            let offset = -child.position.as_ivec2();
            child.inner.on_input(event.offset(offset));
        }
    }
}

impl Flow {
    pub fn new(dir: FlowDirection) -> Self {
        Self {
            dir,
            size: UVec2::ZERO,
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: impl Widget) -> Self {
        self.children.push(Child::new(child));
        self
    }
}

pub struct Label {
    font: Arc<bdf::Font>,
    content: String,
}

impl Label {
    pub fn new(font: Arc<bdf::Font>, content: String) -> Self {
        Self { font, content }
    }

    pub fn draw(&self, dt: &mut DrawTarget) {
        let mut cursor = 0;
        for c in self.content.chars() {
            let Some(glyph) = self.font.glyphs().get(&c) else {
                continue;
            };

            let (mut ox, mut oy) = glyph
                .vector()
                .map(|(x, y)| (*x as i32, *y as i32))
                .unwrap_or((glyph.bounds().x, glyph.bounds().y));

            ox += cursor;
            oy += glyph.bounds().height as i32 - self.font.bounds().height as i32 + 10;

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

            cursor += glyph
                .device_width()
                .map(|w| w.0 as i32)
                .unwrap_or(glyph.width() as i32 + 1);
        }
    }
}

/// A UI slider object
struct Slider {
    track_size: Vec2,
    track_source: Source<'static>,
    handle_pos: i32,
    handle_grab: Option<i32>,
    handle_size: Vec2,
    handle_source: Source<'static>,
    handle_grab_source: Source<'static>,
}

impl Default for Slider {
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

impl Widget for Slider {
    fn layout(&mut self, _constraints: &Constraints) -> UVec2 {
        uvec2(21, 108)
    }

    fn draw(&self) -> Pixels {
        let mut dt = DrawTarget::new(21, 108);
        dt.set_transform(&Transform::translation(11.0, 3.0));

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

        dt_to_pixels(&dt)
    }

    fn on_input(&mut self, event: InputEvent) {
        use InputEvent::*;
        match event.offset(ivec2(-11, -2)) {
            DragStart(pos) => {
                let handle_pos = IVec2::new(0, self.handle_pos);
                let pos = ((pos - handle_pos).abs() * 2).as_vec2();

                if pos.x > self.handle_size.x || pos.y > self.handle_size.y {
                    return;
                }

                self.handle_grab = Some(self.handle_pos);
            }
            DragEnd => {
                self.handle_grab = None;
            }
            DragMove(delta) => {
                if let Some(grab) = self.handle_grab.as_ref() {
                    self.handle_pos = *grab + delta.y;
                    self.handle_pos = self
                        .handle_pos
                        .clamp(1, self.track_size.y.ceil() as i32 - 1);
                }
            }
        }
    }
}

/// A container widget that acts as the root of a fixed-size UI.
pub struct Screen {
    child: Child,
    size: UVec2,
}

impl Widget for Screen {
    fn layout(&mut self, _constraints: &Constraints) -> UVec2 {
        panic!("can't layout screen widget");
    }

    fn draw(&self) -> Pixels {
        let mut dt = DrawTarget::new(self.size.x as i32, self.size.y as i32);

        dt.clear(SolidSource::from_unpremultiplied_argb(
            0xff, 0xd6, 0xf4, 0xfe,
        ));

        self.child.draw(&mut dt);
        dt_to_pixels(&dt)
    }

    fn on_input(&mut self, event: InputEvent) {
        self.child
            .inner
            .on_input(event.offset(-self.child.position.as_ivec2()));
    }
}

impl Screen {
    pub fn new(mut child: impl Widget, size: UVec2) -> Self {
        let child_size = child.layout(&Constraints {
            min: UVec2::ZERO,
            max: size,
        });

        let mut child = Child::new(child);
        child.position = (size - child_size) / 2;

        Self { child, size }
    }
}

/// Helper function to initialize raqote Source from RGB values
fn source_from_rgb(r: u8, g: u8, b: u8) -> Source<'static> {
    Source::Solid(SolidSource::from_unpremultiplied_argb(255, r, g, b))
}

#[no_mangle]
pub extern "C" fn run() {
    let events = MAIN_WINDOW.subscribe();
    MAIN_WINDOW.set_camera(90.0, 0.01, Default::default());

    let canvas = Canvas::new(
        Position {
            origin: vec3(0.0, 0.0, -1.0),
            orientation: Default::default(),
            half_size: vec2(1.0, 1.0),
        },
        Pixels {
            width: 1,
            height: 1,
            data: vec![0xff; 4],
        },
        CanvasSamplingMode::Nearest,
    );
    let canvas_size = (200, 200);
    let mut dt = DrawTarget::new(canvas_size.0, canvas_size.1);
    let mut window_size = Vec2::new(10.0, 10.0);
    let mut slider = Slider::default();
    let mut cursor_pos = Vec2::ZERO;
    let slider_pos = Vec2::new(100.0, 50.0);
    let mut grab_start: Option<Vec2> = None;

    let font = Arc::new(bdf::read(include_bytes!("cozette/cozette.bdf").as_slice()).unwrap());

    let label = Label::new(font.clone(), "Hello, Hearth! ♡".into());
    let max = Label::new(font.clone(), "Max".into());
    let zero = Label::new(font.clone(), "0".into());
    let min = Label::new(font.clone(), "Min".into());

    let mut row = Flow::new(FlowDirection::Horizontal);

    for _ in 0..5 {
        row = row.child(Slider::default());
    }

    let mut root = Screen::new(row, uvec2(canvas_size.0 as u32, canvas_size.1 as u32));

    let mut redraw = true;
    loop {
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
                        let delta = (cursor_pos - *start).as_ivec2();
                        root.on_input(InputEvent::DragMove(delta));
                        redraw = true;
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                } => {
                    grab_start = Some(cursor_pos);
                    root.on_input(InputEvent::DragStart(cursor_pos.as_ivec2()));
                    redraw = true;
                }
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                } => {
                    grab_start = None;
                    root.on_input(InputEvent::DragEnd);
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

        canvas.update(root.draw());
        redraw = false;

        /*dt.clear(SolidSource::from_unpremultiplied_argb(
            0xff, 0xd6, 0xf4, 0xfe,
        ));

        let translate = Transform::translation;

        dt.set_transform(&translate(slider_pos.x, slider_pos.y));
        slider.draw(&mut dt);

        dt.set_transform(&translate(10.0, 15.0));
        label.draw(&mut dt);

        let label_x = slider_pos.x - 40.0;

        dt.set_transform(&translate(label_x, slider_pos.y));
        max.draw(&mut dt);

        dt.set_transform(&translate(label_x, slider_pos.y + 50.0));
        zero.draw(&mut dt);

        dt.set_transform(&translate(label_x, slider_pos.y + 100.0));
        min.draw(&mut dt);

        canvas.update(dt_to_pixels(&dt));
        redraw = false;*/
    }
}
