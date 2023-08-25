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

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use alacritty_terminal::term::color::{Colors, Rgb};
use glam::Vec2;
use rend3::types::TextureHandle;
use rend3_alacritty::terminal::{Terminal, TerminalConfig, TerminalState};
use rend3_alacritty::text::{FaceAtlas, FontSet};
use rend3_alacritty::TerminalStore;
use rend3_routine::base::BaseRenderGraphIntermediateState;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ControlFlow;

const SAMPLE_COUNT: rend3::types::SampleCount = rend3::types::SampleCount::One;

fn load_skybox_image(data: &mut Vec<u8>, image: &[u8]) {
    let decoded = image::load_from_memory(image).unwrap().into_rgba8();
    data.extend_from_slice(decoded.as_raw());
}

pub struct DemoInner {
    store: TerminalStore,
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
    pub fn new(
        renderer: &Arc<rend3::Renderer>,
        surface_format: rend3::types::TextureFormat,
    ) -> Self {
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

        let mut colors = Colors::default();
        Self::load_colors(&mut colors);

        let command: String = match std::env::consts::OS {
            "dragonfly" | "freebsd" | "haiku" | "linux" | "macos" | "netbsd" | "openbsd"
            | "redox" | "solaris" | "unix" => {
                std::env::var("SHELL").expect("Couldn't get system shell: `$SHELL` not set. ")
            }
            "windows" => {
                std::env::var("COMSPEC").expect("Couldn't get system shell: `%COMSPEC%` not set. ")
            }
            _ => todo!(),
        };

        let state = TerminalState {
            position: glam::Vec3::ZERO,
            orientation: glam::Quat::IDENTITY,
            half_size: Vec2::new(1.2, 0.9),
            opacity: 0.8,
        };

        let config = TerminalConfig {
            fonts,
            colors,
            command,
        };
        let terminal = Terminal::new(config.clone(), state.clone());
        let mut store = TerminalStore::new(config, renderer, surface_format);
        store.insert_terminal(&terminal);

        // load skybox
        let mut data = Vec::new();
        load_skybox_image(&mut data, include_bytes!("skybox/right.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/left.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/top.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/bottom.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/front.jpg"));
        load_skybox_image(&mut data, include_bytes!("skybox/back.jpg"));

        let skybox = renderer.add_texture_cube(rend3::types::Texture {
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            size: (2048, 2048).into(),
            data,
            label: Some("skybox".into()),
            mip_count: rend3::types::MipmapCount::ONE,
            mip_source: rend3::types::MipmapSource::Uploaded,
        });

        Self {
            terminal,
            store,
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

    pub fn load_colors(color: &mut Colors) {
        use alacritty_terminal::ansi::NamedColor::*;

        let maps = [
            (Black, Rgb { r: 0, g: 0, b: 0 }),
            (Red, Rgb { r: 255, g: 0, b: 0 }),
            (Green, Rgb { r: 0, g: 255, b: 0 }),
            (Blue, Rgb { r: 0, g: 0, b: 255 }),
            (
                Yellow,
                Rgb {
                    r: 255,
                    g: 255,
                    b: 0,
                },
            ),
            (
                Magenta,
                Rgb {
                    r: 255,
                    g: 0,
                    b: 255,
                },
            ),
            (
                Cyan,
                Rgb {
                    r: 0,
                    g: 255,
                    b: 255,
                },
            ),
            (
                White,
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ),
        ];

        for map in maps.iter() {
            color[map.0] = Some(map.1);
        }

        let dupes = [
            (Background, Black),
            (Foreground, White),
            (BrightBlack, Black),
            (BrightRed, Red),
            (BrightGreen, Green),
            (BrightYellow, Yellow),
            (BrightBlue, Blue),
            (BrightMagenta, Magenta),
            (BrightCyan, Cyan),
            (BrightWhite, White),
        ];

        for (dst, src) in dupes.iter() {
            color[*dst] = color[*src];
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
    const HANDEDNESS: rend3::types::Handedness = rend3::types::Handedness::Right;

    fn sample_count(&self) -> rend3::types::SampleCount {
        SAMPLE_COUNT
    }

    fn setup(
        &mut self,
        _window: &winit::window::Window,
        renderer: &Arc<rend3::Renderer>,
        routines: &Arc<rend3_framework::DefaultRoutines>,
        surface_format: rend3::types::TextureFormat,
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
        renderer: &Arc<rend3::Renderer>,
        routines: &Arc<rend3_framework::DefaultRoutines>,
        base_rendergraph: &rend3_routine::base::BaseRenderGraph,
        surface: Option<&Arc<rend3::types::Surface>>,
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

                renderer.set_camera_data(rend3::types::Camera {
                    projection: rend3::types::CameraProjection::Perspective {
                        vfov: 60.0,
                        near: 0.1,
                    },
                    view: glam::Mat4::look_at_rh(
                        eye,
                        glam::Vec3::ZERO,
                        glam::Vec3::new(0.0, 1.0, 0.0),
                    ),
                });

                let frame = rend3::util::output::OutputFrame::Surface {
                    surface: Arc::clone(surface.unwrap()),
                };

                let routine = inner.store.create_routine();

                let pbr_routine = rend3_framework::lock(&routines.pbr);
                let mut skybox_routine = rend3_framework::lock(&routines.skybox);
                let tonemapping_routine = rend3_framework::lock(&routines.tonemapping);

                let (cmd_bufs, ready) = renderer.ready();
                skybox_routine.ready(renderer);

                let mut graph = rend3::graph::RenderGraph::new();

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

                let depth = state.depth;
                let output = graph.add_surface_texture();
                routine.add_to_graph(&mut graph, output, depth);

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
        winit::window::WindowBuilder::new().with_title("rend3-alacritty demo"),
    );
}
