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

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use glam::Vec2;
use hearth_rend3::rend3::graph::RenderGraph;
use hearth_rend3::rend3::util::output::OutputFrame;
use hearth_rend3::rend3::{types::*, Renderer};
use hearth_rend3::rend3_routine::base::{BaseRenderGraph, BaseRenderGraphIntermediateState};
use hearth_rend3::wgpu::{self, TextureFormat};
use hearth_schema::terminal::TerminalState;
use hearth_schema::Color;
use hearth_terminal::draw::{TerminalDrawState, TerminalPipelines};
use hearth_terminal::terminal::{Terminal, TerminalConfig};
use hearth_terminal::text::{FaceAtlas, FontSet};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ControlFlow;

const SAMPLE_COUNT: SampleCount = SampleCount::One;

fn load_skybox_image(data: &mut Vec<u8>, image: &[u8]) {
    let decoded = image::load_from_memory(image).unwrap().into_rgba8();
    data.extend_from_slice(decoded.as_raw());
}

pub struct DemoInner {
    pipelines: TerminalPipelines,
    draw_state: TerminalDrawState,
    terminal: Arc<Terminal>,
    skybox: TextureHandle,
    orbit_pitch: f32,
    orbit_yaw: f32,
    orbit_distance: f32,
    mouse_pos: PhysicalPosition<f64>,
    is_orbiting: bool,
    state: TerminalState,
    is_resizing: bool,
}

impl DemoInner {
    pub fn new(renderer: &Arc<Renderer>, surface_format: TextureFormat) -> Self {
        let ttf_srcs = FontSet {
            regular: include_bytes!("../../../resources/mononoki/mononoki-Regular.ttf").to_vec(),
            italic: include_bytes!("../../../resources/mononoki/mononoki-Italic.ttf").to_vec(),
            bold: include_bytes!("../../../resources/mononoki/mononoki-Bold.ttf").to_vec(),
            bold_italic: include_bytes!("../../../resources/mononoki/mononoki-BoldItalic.ttf")
                .to_vec(),
        };

        let fonts = ttf_srcs.map(|src| {
            let face = owned_ttf_parser::OwnedFace::from_vec(src, 0).unwrap();
            let face_atlas = FaceAtlas::new(face, &renderer.device, renderer.queue.to_owned());
            Arc::new(face_atlas)
        });

        let c = Color::from_rgb;

        let colors = FromIterator::from_iter([
            (0x0, c(0, 0, 0)),         // black
            (0x1, c(187, 0, 0)),       // red
            (0x2, c(0, 187, 0)),       // green
            (0x3, c(187, 187, 0)),     // yellow
            (0x4, c(0, 0, 187)),       // blue
            (0x5, c(187, 0, 187)),     // magenta
            (0x6, c(0, 187, 187)),     // cyan
            (0x7, c(187, 187, 187)),   // white
            (0x8, c(85, 85, 85)),      // bright black
            (0x9, c(255, 85, 85)),     // bright red
            (0xA, c(85, 255, 85)),     // bright green
            (0xB, c(255, 255, 85)),    // bright yellow
            (0xC, c(85, 85, 255)),     // bright blue
            (0xD, c(255, 85, 255)),    // bright magenta
            (0xE, c(85, 255, 255)),    // bright cyan
            (0xF, c(255, 255, 255)),   // bright white
            (0x100, c(255, 255, 255)), // foreground
            (0x101, c(0, 0, 0)),       // background
        ]);

        let state = TerminalState {
            position: glam::Vec3::ZERO,
            orientation: glam::Quat::IDENTITY,
            half_size: Vec2::new(1.2, 0.9),
            padding: Vec2::splat(0.2),
            opacity: 0.95,
            units_per_em: 0.04,
            colors,
        };

        let pipelines = TerminalPipelines::new(
            renderer.device.clone(),
            renderer.queue.clone(),
            surface_format,
        );

        let command = None; // autoselect shell
        let config = TerminalConfig { fonts, command };
        let terminal = Terminal::new(config.clone(), state.clone());
        let draw_state = TerminalDrawState::new(&pipelines, terminal.get_fonts());

        // load skybox
        let mut data = Vec::new();
        load_skybox_image(&mut data, include_bytes!("skybox/right.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/left.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/top.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/bottom.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/front.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/back.jpg"));

        let skybox = renderer.add_texture_cube(Texture {
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            size: (2048, 2048).into(),
            data,
            label: Some("skybox".into()),
            mip_count: MipmapCount::ONE,
            mip_source: MipmapSource::Uploaded,
        });

        Self {
            pipelines,
            draw_state,
            terminal,
            skybox,
            orbit_pitch: 0.0,
            orbit_yaw: 0.0,
            orbit_distance: 3.0,
            state,
            is_orbiting: false,
            is_resizing: false,
            mouse_pos: Default::default(),
        }
    }

    pub fn virtual_keycode_to_string(
        keycode: winit::event::VirtualKeyCode,
    ) -> Option<&'static str> {
        use winit::event::VirtualKeyCode::*;
        match keycode {
            Back => Some("\x7f"),
            Up => Some("\x1b[A"),
            Down => Some("\x1b[B"),
            Right => Some("\x1b[C"),
            Left => Some("\x1b[D"),
            Home => Some("\x1b[1~"),
            Insert => Some("\x1b[2~"),
            Delete => Some("\x1b[3~"),
            End => Some("\x1b[4~"),
            PageUp => Some("\x1b[5~"),
            PageDown => Some("\x1b[6~"),
            _ => None,
        }
    }

    pub fn on_keyboard_input(&mut self, input: &winit::event::KeyboardInput) {
        if input.state == winit::event::ElementState::Pressed {
            if let Some(keycode) = input.virtual_keycode {
                if let Some(input) = Self::virtual_keycode_to_string(keycode) {
                    self.terminal.send_input(input);
                }
            }
        }
    }

    pub fn on_received_character(&mut self, c: char) {
        match c {
            '\u{7f}' | '\u{8}' => {
                // We use a special escape code for the delete and backspace keys.
                return;
            }
            _ => {}
        }

        let string = c.to_string();
        self.terminal.send_input(string.as_str());
    }
}

#[derive(Default)]
pub struct Demo {
    inner: Option<DemoInner>,
}

impl rend3_framework::App for Demo {
    const HANDEDNESS: Handedness = Handedness::Right;

    fn sample_count(&self) -> SampleCount {
        SAMPLE_COUNT
    }

    fn setup(
        &mut self,
        _window: &winit::window::Window,
        renderer: &Arc<Renderer>,
        routines: &Arc<rend3_framework::DefaultRoutines>,
        surface_format: TextureFormat,
    ) {
        let inner = DemoInner::new(renderer, surface_format);

        routines
            .skybox
            .lock()
            .set_background_texture(Some(inner.skybox.clone()));

        self.inner = Some(inner);
    }

    fn handle_event(
        &mut self,
        window: &winit::window::Window,
        renderer: &Arc<Renderer>,
        routines: &Arc<rend3_framework::DefaultRoutines>,
        base_rendergraph: &BaseRenderGraph,
        surface: Option<&Arc<Surface>>,
        resolution: glam::UVec2,
        event: rend3_framework::Event<'_, ()>,
        control_flow: impl FnOnce(winit::event_loop::ControlFlow),
    ) {
        let inner = self.inner.as_mut().unwrap();
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    control_flow(ControlFlow::Exit);
                }

                WindowEvent::KeyboardInput { input, .. } => {
                    inner.on_keyboard_input(&input);
                }
                WindowEvent::ReceivedCharacter(c) => {
                    inner.on_received_character(c);
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => match state {
                    ElementState::Pressed => inner.is_orbiting = true,
                    ElementState::Released => inner.is_orbiting = false,
                },
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Right,
                    ..
                } => match state {
                    ElementState::Pressed => inner.is_resizing = true,
                    ElementState::Released => inner.is_resizing = false,
                },
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Middle,
                    ..
                } => {
                    inner.orbit_pitch = 0.0;
                    inner.orbit_yaw = 0.0;
                    inner.orbit_distance = 3.0;
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let dx = (position.x - inner.mouse_pos.x) as f32;
                    let dy = (position.y - inner.mouse_pos.y) as f32;
                    inner.mouse_pos = position;

                    if inner.is_orbiting {
                        let yaw_factor = -0.003;
                        let pitch_factor = -0.003;

                        inner.orbit_yaw += dx * yaw_factor;

                        inner.orbit_pitch = (inner.orbit_pitch + dy * pitch_factor)
                            .clamp(-FRAC_PI_2 * 0.99, FRAC_PI_2 * 0.99);
                    } else if inner.is_resizing {
                        let factor = 0.01;
                        inner.state.half_size = (inner.state.half_size
                            + Vec2::new(dx, dy) * factor)
                            .clamp(Vec2::new(0.1, 0.1), Vec2::new(4.0, 4.0));

                        inner.terminal.update(inner.state.clone());
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let delta = match delta {
                        MouseScrollDelta::LineDelta(_hori, vert) => vert * 30.0,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                    };

                    let factor = -0.01;
                    inner.orbit_distance = (inner.orbit_distance + delta * factor).clamp(1.0, 20.0);
                }
                _ => {}
            },
            Event::MainEventsCleared => {
                if inner.terminal.should_quit() {
                    control_flow(ControlFlow::Exit);
                } else {
                    window.request_redraw();
                }
            }
            Event::RedrawRequested(_) => {
                let eye = glam::Quat::from_rotation_y(inner.orbit_yaw)
                    * glam::Quat::from_rotation_x(inner.orbit_pitch)
                    * (glam::Vec3::Z * inner.orbit_distance);

                renderer.set_camera_data(Camera {
                    projection: CameraProjection::Perspective {
                        vfov: 60.0,
                        near: 0.1,
                    },
                    view: glam::Mat4::look_at_rh(
                        eye,
                        glam::Vec3::ZERO,
                        glam::Vec3::new(0.0, 1.0, 0.0),
                    ),
                });

                let frame = OutputFrame::Surface {
                    surface: Arc::clone(surface.unwrap()),
                };

                inner
                    .terminal
                    .update_draw_state(&inner.pipelines, &mut inner.draw_state);

                let pbr_routine = rend3_framework::lock(&routines.pbr);
                let mut skybox_routine = rend3_framework::lock(&routines.skybox);
                let tonemapping_routine = rend3_framework::lock(&routines.tonemapping);

                let (cmd_bufs, ready) = renderer.ready();
                skybox_routine.ready(renderer);

                let mut graph = RenderGraph::new();

                base_rendergraph.add_to_graph(
                    &mut graph,
                    &ready,
                    &pbr_routine,
                    Some(&skybox_routine),
                    &tonemapping_routine,
                    resolution,
                    SAMPLE_COUNT,
                    glam::Vec4::ZERO,
                );

                let state = BaseRenderGraphIntermediateState::new(
                    &mut graph,
                    &ready,
                    resolution,
                    SAMPLE_COUNT,
                );

                let draws = &[&inner.draw_state];
                let output = graph.add_surface_texture();
                inner
                    .pipelines
                    .add_to_graph(draws, &mut graph, output, state.depth);

                graph.execute(renderer, frame, cmd_bufs, &ready);
            }
            _ => {}
        }
    }
}

fn main() {
    let app = Demo::default();
    rend3_framework::start(
        app,
        winit::window::WindowBuilder::new().with_title("Hearth Terminal Emulator Demo"),
    );
}
