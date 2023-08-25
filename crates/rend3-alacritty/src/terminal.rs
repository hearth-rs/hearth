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

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Sender},
        Arc,
    },
    thread::JoinHandle,
};

use alacritty_terminal::{
    ansi::{Color, CursorShape, NamedColor},
    config::PtyConfig,
    event::{Event, EventListener},
    event_loop::{EventLoop, Msg, State},
    grid::Indexed,
    sync::FairMutex,
    term::{
        cell::{Cell, Flags},
        color::{Colors, Rgb, COUNT},
        RenderableContent, RenderableCursor,
    },
    tty::Pty,
    Term,
};
use glam::{IVec2, Mat4, Quat, UVec2, Vec2, Vec3};
use mio_extras::channel::Sender as MioSender;
use owned_ttf_parser::AsFaceRef;
use wgpu::{
    self, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferAddress,
    BufferDescriptor, BufferUsages, Device,
};

use crate::{
    gpu::{CameraUniform, DynamicMesh, GlyphVertex, SolidVertex},
    text::{FaceAtlas, FontSet, FontStyle},
};

pub struct Listener {
    sender: Sender<Event>,
}

impl Listener {
    pub fn new(sender: Sender<Event>) -> Self {
        Self { sender }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.sender.send(event).unwrap();
    }
}

/// Configuration for the initialization of a terminal.
#[derive(Clone)]
pub struct TerminalConfig {
    pub fonts: FontSet<Arc<FaceAtlas>>,
    pub colors: Colors,
    pub command: String,
}

/// Dynamic terminal appearance and settings configuration.
#[derive(Clone, Debug)]
pub struct TerminalState {
    pub position: Vec3,
    pub orientation: Quat,
    pub half_size: Vec2,
    pub opacity: f32,
}

/// Private terminal mutable state.
struct TerminalInner {
    grid_size: UVec2,
    state: TerminalState,
}

/// A CPU-side wrapper around terminal functionality.
pub struct Terminal {
    config: TerminalConfig,
    term: Arc<FairMutex<Term<Listener>>>,
    _term_loop: JoinHandle<(EventLoop<Pty, Listener>, State)>,
    term_channel: FairMutex<MioSender<Msg>>,
    should_quit: AtomicBool,
    inner: FairMutex<TerminalInner>,
    units_per_em: f32,
    font_baselines: FontSet<f32>,
    cell_size: Vec2,
}

impl Terminal {
    pub fn new(config: TerminalConfig, initial_state: TerminalState) -> Arc<Self> {
        let face_metrics = config.fonts.as_ref().map(|face| {
            let face = face.face.as_face_ref();
            let units_per_em = face.units_per_em() as f32;
            let ascender = face.ascender() as f32 / units_per_em;
            let height = face.height() as f32 / units_per_em;
            let descender = face.descender() as f32 / units_per_em;
            let height = height.max(ascender + descender);
            (ascender, height, descender)
        });

        let cell_height = face_metrics.regular.1;

        let face = config.fonts.regular.face.as_face_ref();
        let units_per_em = face.units_per_em() as f32;
        let cell_width = face
            .glyph_index('M')
            .and_then(|id| face.glyph_hor_advance(id))
            .map(|adv| adv as f32 / units_per_em)
            .unwrap_or(1.0);

        let font_baselines = face_metrics
            .map(|(ascender, height, _descender)| (cell_height - height) / 2.0 + ascender);

        let cell_size = Vec2::new(cell_width, cell_height);

        let units_per_em = 0.04;
        let grid_size = (initial_state.half_size * 2.0 / cell_size / units_per_em)
            .ceil()
            .as_uvec2();

        let size_info = alacritty_terminal::term::SizeInfo::new(
            grid_size.x as f32,
            grid_size.y as f32,
            1.0,
            1.0,
            0.0,
            0.0,
            false,
        );

        let (sender, term_events) = channel();

        let shell = alacritty_terminal::config::Program::Just(config.command.clone().into());

        let term_config = alacritty_terminal::config::Config {
            pty_config: PtyConfig {
                shell: Some(shell),
                working_directory: None,
                hold: false,
            },
            ..Default::default()
        };

        let term_listener = Listener::new(sender.clone());

        let term = Term::new(&term_config, size_info, term_listener);
        let term = FairMutex::new(term);
        let term = Arc::new(term);

        let pty = alacritty_terminal::tty::new(&term_config.pty_config, &size_info, None).unwrap();

        let term_listener = Listener::new(sender);
        let term_loop = EventLoop::new(term.clone(), term_listener, pty, false, false);
        let term_channel = term_loop.channel();

        let inner = TerminalInner {
            grid_size,
            state: initial_state,
        };

        let term = Self {
            config,
            term,
            _term_loop: term_loop.spawn(),
            term_channel: FairMutex::new(term_channel),
            should_quit: AtomicBool::new(false),
            inner: FairMutex::new(inner),
            units_per_em,
            cell_size,
            font_baselines,
        };

        let term = Arc::new(term);

        let event_term = term.to_owned();
        std::thread::spawn(move || {
            while let Ok(event) = term_events.recv() {
                event_term.on_event(event);
            }
        });

        term
    }

    pub fn update(&self, state: TerminalState) {
        let mut inner = self.inner.lock();

        let grid_size = (state.half_size * 2.0 / self.cell_size / self.units_per_em)
            .floor()
            .as_uvec2();

        if inner.grid_size != grid_size {
            inner.grid_size = grid_size;

            let size_info = alacritty_terminal::term::SizeInfo::new(
                grid_size.x as f32,
                grid_size.y as f32,
                1.0,
                1.0,
                0.0,
                0.0,
                false,
            );

            self.term_channel
                .lock()
                .send(Msg::Resize(size_info))
                .unwrap();

            self.term.lock().resize(size_info);
        }

        inner.state = state;
    }

    pub fn update_draw_state(&self, draw: &mut TerminalDrawState) {
        let inner = self.inner.lock();
        let grid_size = inner.grid_size;
        let state = inner.state.clone();
        drop(inner); // get off the mutex

        let colors = self.config.colors.clone();
        let fonts = self.config.fonts.clone();
        let font_baselines = self.font_baselines.clone();
        let units_per_em = self.units_per_em;
        let mut canvas = TerminalCanvas::new(
            colors,
            fonts,
            units_per_em,
            state,
            grid_size,
            self.cell_size,
            font_baselines,
        );

        let term = self.term.lock();
        let content = term.renderable_content();
        canvas.update_from_content(content);
        drop(term); // get off the mutex

        canvas.apply_to_state(draw);
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit.load(Ordering::Relaxed)
    }

    pub fn send_input(&self, input: &str) {
        let bytes = input.as_bytes();
        let cow = std::borrow::Cow::Owned(bytes.to_owned());
        self.term_channel.lock().send(Msg::Input(cow)).unwrap();
    }

    fn on_event(&self, event: Event) {
        match event {
            Event::ColorRequest(index, format) => {
                let color = self.config.colors[index].unwrap_or(Rgb {
                    r: 0xff,
                    g: 0xff,
                    b: 0xff,
                });

                self.send_input(&format(color));
            }
            Event::PtyWrite(text) => self.send_input(&text),
            Event::Exit => self.should_quit.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
}

/// A ready-to-render terminal state.
pub struct TerminalDrawState {
    pub device: Arc<Device>,
    pub model: Mat4,
    pub camera_buffer: Buffer,
    pub camera_bind_group: BindGroup,
    pub bg_mesh: DynamicMesh<SolidVertex>,
    pub glyph_meshes: FontSet<DynamicMesh<GlyphVertex>>,
    pub overlay_mesh: DynamicMesh<SolidVertex>,
}

impl TerminalDrawState {
    pub fn new(device: Arc<Device>, camera_bgl: &BindGroupLayout) -> Self {
        let camera_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Alacritty terminal camera buffer"),
            size: std::mem::size_of::<CameraUniform>() as BufferAddress,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Alacritty terminal camera bind group"),
            layout: camera_bgl,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        Self {
            model: Mat4::IDENTITY,
            camera_buffer,
            camera_bind_group,
            bg_mesh: DynamicMesh::new(&device),
            glyph_meshes: FontSet {
                regular: DynamicMesh::new(&device),
                italic: DynamicMesh::new(&device),
                bold: DynamicMesh::new(&device),
                bold_italic: DynamicMesh::new(&device),
            },
            overlay_mesh: DynamicMesh::new(&device),
            device,
        }
    }
}

/// An in-progress terminal draw state.
pub struct TerminalCanvas {
    colors: Colors,
    fonts: FontSet<Arc<FaceAtlas>>,
    bg_vertices: Vec<SolidVertex>,
    bg_indices: Vec<u32>,
    overlay_vertices: Vec<SolidVertex>,
    overlay_indices: Vec<u32>,
    glyphs: Vec<(Vec2, FontStyle, u16, u32)>,
    units_per_em: f32,
    state: TerminalState,
    grid_size: UVec2,
    cell_size: Vec2,
    font_baselines: FontSet<f32>,
}

impl TerminalCanvas {
    pub fn new(
        colors: Colors,
        fonts: FontSet<Arc<FaceAtlas>>,
        units_per_em: f32,
        state: TerminalState,
        grid_size: UVec2,
        cell_size: Vec2,
        font_baselines: FontSet<f32>,
    ) -> Self {
        Self {
            colors,
            fonts,
            bg_vertices: Vec::new(),
            bg_indices: Vec::new(),
            overlay_vertices: Vec::new(),
            overlay_indices: Vec::new(),
            glyphs: Vec::new(),
            units_per_em,
            state,
            grid_size,
            cell_size,
            font_baselines,
        }
    }

    pub fn update_from_content(&mut self, content: RenderableContent) {
        for index in 0..COUNT {
            if let Some(color) = content.colors[index] {
                self.colors[index] = Some(color);
            }
        }

        for cell in content.display_iter {
            self.draw_cell(cell);
        }

        self.draw_cursor(content.cursor);
    }

    pub fn apply_to_state(&self, state: &mut TerminalDrawState) {
        let mut touched = FontSet::<Vec<u16>>::default();
        let mut glyph_meshes = FontSet::<(Vec<GlyphVertex>, Vec<u32>)>::default();

        for (offset, style, glyph, color) in self.glyphs.iter().copied() {
            let (vertices, indices) = &mut glyph_meshes.get_mut(style);
            let baseline = *self.font_baselines.get(style) * self.units_per_em;
            let offset = offset + Vec2::new(0.0, -baseline);

            let index = vertices.len() as u32;
            let atlas = &self.fonts.get(style).atlas;
            let bitmap = match atlas.glyphs[glyph as usize].as_ref() {
                Some(b) => b,
                None => continue,
            };

            touched.get_mut(style).push(glyph);

            vertices.extend(bitmap.vertices.iter().map(|v| GlyphVertex {
                position: v.position * self.units_per_em + offset,
                tex_coords: v.tex_coords,
                color,
            }));

            indices.extend_from_slice(&[
                index,
                index + 1,
                index + 2,
                index + 2,
                index + 1,
                index + 3,
            ]);
        }

        self.fonts
            .as_ref()
            .zip(touched)
            .for_each(|(font, touched)| {
                font.touch(&touched);
            });

        state
            .glyph_meshes
            .as_mut()
            .zip(glyph_meshes)
            .for_each(|(mesh, (vertices, indices))| {
                mesh.update(&state.device, &vertices, &indices)
            });

        state
            .bg_mesh
            .update(&state.device, &self.bg_vertices, &self.bg_indices);

        state
            .overlay_mesh
            .update(&state.device, &self.overlay_vertices, &self.overlay_indices);

        state.model =
            Mat4::from_translation(self.state.position) * Mat4::from_quat(self.state.orientation);
    }

    pub fn draw_cell(&mut self, cell: Indexed<&Cell>) {
        if cell.flags.contains(Flags::HIDDEN) {
            return;
        }

        let col = cell.point.column.0 as i32;
        let row = cell.point.line.0;
        let pos = self.grid_to_pos(col, row);
        let mut fg = cell.fg;
        let mut bg = cell.bg;

        let is_full_block = cell.c == '▀';

        if cell.flags.contains(Flags::INVERSE) ^ is_full_block {
            std::mem::swap(&mut fg, &mut bg);
        }

        if !is_full_block {
            let style = FontStyle::from_cell_flags(cell.flags);
            let face = self.fonts.get(style).face.as_face_ref();
            if let Some(glyph) = face.glyph_index(cell.c) {
                let fg = self.color_to_u32(fg);
                self.glyphs.push((pos, style, glyph.0, fg));
            }
        }

        let bg = if bg == Color::Named(NamedColor::Background) {
            let base = self.color_to_u32(bg);
            let alpha = (self.state.opacity * 255.0) as u8;
            ((alpha as u32) << 24) | (base & 0x00ffffff)
        } else {
            self.color_to_u32(bg)
        };

        let tl = self.grid_to_pos(col, row);
        let br = self.grid_to_pos(col + 1, row + 1);
        self.draw_solid_rect(tl, br, bg);
    }

    pub fn draw_cursor(&mut self, cursor: RenderableCursor) {
        let cursor_color = Color::Named(NamedColor::Foreground);
        let cursor_color = self.color_to_u32(cursor_color);
        let col = cursor.point.column.0 as i32;
        let row = cursor.point.line.0;
        match cursor.shape {
            CursorShape::Hidden => {}
            _ => {
                let tl = self.grid_to_pos(col, row);
                let br = self.grid_to_pos(col + 1, row + 1);
                self.draw_solid_rect(tl, br, cursor_color);
            }
        }
    }

    pub fn draw_solid_rect(&mut self, tl: Vec2, br: Vec2, color: u32) {
        let index = self.bg_vertices.len() as u32;
        self.bg_vertices.extend_from_slice(&[
            SolidVertex {
                position: tl,
                color,
            },
            SolidVertex {
                position: Vec2::new(br.x, tl.y),
                color,
            },
            SolidVertex {
                position: Vec2::new(tl.x, br.y),
                color,
            },
            SolidVertex {
                position: br,
                color,
            },
        ]);

        self.bg_indices.extend_from_slice(&[
            index,
            index + 1,
            index + 2,
            index + 2,
            index + 1,
            index + 3,
        ]);
    }

    pub fn grid_to_pos(&self, x: i32, y: i32) -> Vec2 {
        let mut pos = IVec2::new(x, y).as_vec2() - self.grid_size.as_vec2() / 2.0;
        pos.y = -pos.y;
        pos * self.cell_size * self.units_per_em
    }

    pub fn color_to_rgb(&self, color: Color) -> Rgb {
        match color {
            Color::Named(name) => self.colors[name].unwrap(),
            Color::Spec(rgb) => rgb,
            Color::Indexed(index) => {
                if let Some(color) = self.colors[index as usize] {
                    color
                } else if let Some(gray) = index.checked_sub(232) {
                    let value = gray * 10 + 8;
                    Rgb {
                        r: value,
                        g: value,
                        b: value,
                    }
                } else if let Some(cube_idx) = index.checked_sub(16) {
                    let r = cube_idx / 36;
                    let g = (cube_idx / 6) % 6;
                    let b = cube_idx % 6;

                    let c = |c| {
                        if c == 0 {
                            0
                        } else {
                            c * 40 + 55
                        }
                    };

                    Rgb {
                        r: c(r),
                        g: c(g),
                        b: c(b),
                    }
                } else {
                    Rgb {
                        r: 0xff,
                        g: 0x00,
                        b: 0xff,
                    }
                }
            }
        }
    }

    pub fn color_to_u32(&self, color: Color) -> u32 {
        let rgb = self.color_to_rgb(color);
        0xff000000 | ((rgb.b as u32) << 16) | ((rgb.g as u32) << 8) | (rgb.r as u32)
    }
}
