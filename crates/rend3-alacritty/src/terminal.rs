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
use glam::{vec2, IVec2, Mat4, Quat, UVec2, Vec2, Vec3};
use mio_extras::channel::Sender as MioSender;
use owned_ttf_parser::AsFaceRef;
use wgpu::{
    self, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferAddress,
    BufferDescriptor, BufferUsages, Device, Queue,
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

    /// The command that this terminal will run.
    ///
    /// Defaults to a platform-specific shell.
    pub command: Option<String>,
}

impl TerminalConfig {
    fn unwrap_command(&self) -> String {
        match self.command.to_owned() {
            Some(command) => command,
            None => match std::env::consts::OS {
                "dragonfly" | "freebsd" | "haiku" | "linux" | "macos" | "netbsd" | "openbsd"
                | "redox" | "solaris" | "unix" => {
                    std::env::var("SHELL").expect("Couldn't get system shell: `$SHELL` not set. ")
                }
                "windows" => std::env::var("COMSPEC")
                    .expect("Couldn't get system shell: `%COMSPEC%` not set. "),
                _ => todo!("OS {} is unrecognized", std::env::consts::OS),
            },
        }
    }
}

/// Dynamic terminal appearance and settings configuration.
#[derive(Clone)]
pub struct TerminalState {
    pub position: Vec3,
    pub orientation: Quat,
    pub half_size: Vec2,
    pub opacity: f32,
    pub colors: Colors,
    pub padding: Vec2,
    pub units_per_em: f32,
}

impl Default for TerminalState {
    fn default() -> Self {
        use alacritty_terminal::ansi::NamedColor::*;
        let mut colors = Colors::default();

        let maps = [
            (
                Black,
                Rgb {
                    r: 16,
                    g: 16,
                    b: 16,
                },
            ),
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
            colors[map.0] = Some(map.1);
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
            colors[*dst] = colors[*src];
        }

        Self {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            half_size: Vec2::ONE,
            opacity: 1.0,
            colors,
            padding: Vec2::ZERO,
            units_per_em: 0.04,
        }
    }
}

#[derive(Clone)]
pub struct FaceWithMetrics {
    atlas: Arc<FaceAtlas>,
    ascender: f32,
    width: f32,
    height: f32,
    strikeout_pos: f32,
    strikeout_width: f32,
    underline_pos: f32,
    underline_width: f32,
}

impl From<Arc<FaceAtlas>> for FaceWithMetrics {
    fn from(atlas: Arc<FaceAtlas>) -> Self {
        let face = atlas.face.as_face_ref();
        let units_per_em = face.units_per_em() as f32;
        let ascender = face.ascender() as f32 / units_per_em;
        let height = face.height() as f32 / units_per_em;
        let descender = face.descender() as f32 / units_per_em;
        let height = height.max(ascender + descender);
        let width = face
            .glyph_index('M')
            .and_then(|id| face.glyph_hor_advance(id))
            .map(|adv| adv as f32 / units_per_em)
            .unwrap_or(1.0);

        let default_width = 0.06;

        let (strikeout_pos, strikeout_width) = face
            .strikeout_metrics()
            .map(|m| (vec2(m.position as f32, m.thickness as f32) / units_per_em).into())
            .unwrap_or((height / 2.0, default_width));

        let (underline_pos, underline_width) = face
            .underline_metrics()
            .map(|m| (vec2(m.position as f32, m.thickness as f32) / units_per_em).into())
            .unwrap_or((height / 2.0, default_width));

        Self {
            atlas,
            ascender,
            height,
            width,
            strikeout_pos,
            strikeout_width,
            underline_pos,
            underline_width,
        }
    }
}

/// Private terminal mutable state.
struct TerminalInner {
    grid_size: UVec2,
    state: TerminalState,
}

/// A CPU-side wrapper around terminal functionality.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    _term_loop: JoinHandle<(EventLoop<Pty, Listener>, State)>,
    term_channel: FairMutex<MioSender<Msg>>,
    should_quit: AtomicBool,
    inner: FairMutex<TerminalInner>,
    fonts: FontSet<FaceWithMetrics>,
    font_baselines: FontSet<f32>,
    cell_size: Vec2,
}

impl Terminal {
    pub fn new(config: TerminalConfig, initial_state: TerminalState) -> Arc<Self> {
        let fonts = config.fonts.clone().map(FaceWithMetrics::from);
        let cell_size = Vec2::new(fonts.regular.width, fonts.regular.height);
        let font_baselines = fonts
            .as_ref()
            .map(|font| (cell_size.y - font.height) / 2.0 + font.ascender);

        let available = (initial_state.half_size - initial_state.padding) * 2.0;
        let grid_size = (available / cell_size / initial_state.units_per_em)
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

        let command = config.unwrap_command();
        let shell = alacritty_terminal::config::Program::Just(command);

        let term_config = alacritty_terminal::config::Config {
            pty_config: PtyConfig {
                shell: Some(shell),
                working_directory: None,
                hold: false,
            },
            ..Default::default()
        };

        // setup environment variables
        alacritty_terminal::tty::setup_env(&term_config);

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
            fonts,
            term,
            _term_loop: term_loop.spawn(),
            term_channel: FairMutex::new(term_channel),
            should_quit: AtomicBool::new(false),
            inner: FairMutex::new(inner),
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

        let available = (state.half_size - state.padding) * 2.0;
        let grid_size = (available / self.cell_size / state.units_per_em)
            .ceil()
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

        let font_baselines = self.font_baselines.clone();
        let mut canvas = TerminalCanvas::new(
            self.fonts.clone(),
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

    pub fn quit(&self) {
        self.should_quit.store(true, Ordering::Relaxed);
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
                let color = self.inner.lock().state.colors[index].unwrap_or(Rgb {
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
    pub queue: Arc<Queue>,
    pub model: Mat4,
    pub camera_buffer: Buffer,
    pub camera_bind_group: BindGroup,
    pub bg_mesh: DynamicMesh<SolidVertex>,
    pub glyph_meshes: FontSet<DynamicMesh<GlyphVertex>>,
    pub overlay_mesh: DynamicMesh<SolidVertex>,
}

impl TerminalDrawState {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, camera_bgl: &BindGroupLayout) -> Self {
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
            bg_mesh: DynamicMesh::new(&device, Some("Alacritty background mesh".into())),
            glyph_meshes: FontSet {
                regular: DynamicMesh::new(&device, Some("Alacritty regular glyph mesh".into())),
                italic: DynamicMesh::new(&device, Some("Alacritty italic glyph mesh".into())),
                bold: DynamicMesh::new(&device, Some("Alacritty bold glyph mesh".into())),
                bold_italic: DynamicMesh::new(
                    &device,
                    Some("Alacritty bold italic glyph mesh".into()),
                ),
            },
            overlay_mesh: DynamicMesh::new(&device, Some("Alacritty overlay mesh".into())),
            device,
            queue,
        }
    }
}

/// An in-progress terminal draw state.
pub struct TerminalCanvas {
    fonts: FontSet<FaceWithMetrics>,
    bg_vertices: Vec<SolidVertex>,
    bg_indices: Vec<u32>,
    overlay_vertices: Vec<SolidVertex>,
    overlay_indices: Vec<u32>,
    glyphs: Vec<(Vec2, FontStyle, u16, u32)>,
    state: TerminalState,
    grid_size: UVec2,
    cell_size: Vec2,
    font_baselines: FontSet<f32>,
}

impl TerminalCanvas {
    pub fn new(
        fonts: FontSet<FaceWithMetrics>,
        state: TerminalState,
        grid_size: UVec2,
        cell_size: Vec2,
        font_baselines: FontSet<f32>,
    ) -> Self {
        Self {
            fonts,
            bg_vertices: Vec::new(),
            bg_indices: Vec::new(),
            overlay_vertices: Vec::new(),
            overlay_indices: Vec::new(),
            glyphs: Vec::new(),
            state,
            grid_size,
            cell_size,
            font_baselines,
        }
    }

    pub fn update_from_content(&mut self, content: RenderableContent) {
        self.draw_padding();

        for index in 0..COUNT {
            if let Some(color) = content.colors[index] {
                self.state.colors[index] = Some(color);
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
            let baseline = *self.font_baselines.get(style) * self.state.units_per_em;
            let offset = offset + Vec2::new(0.0, -baseline);

            let index = vertices.len() as u32;
            let atlas = &self.fonts.get(style).atlas.atlas;
            let bitmap = match atlas.glyphs[glyph as usize].as_ref() {
                Some(b) => b,
                None => continue,
            };

            touched.get_mut(style).push(glyph);

            vertices.extend(bitmap.vertices.iter().map(|v| GlyphVertex {
                position: v.position * self.state.units_per_em + offset,
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
                font.atlas.touch(&touched);
            });

        state
            .glyph_meshes
            .as_mut()
            .zip(glyph_meshes)
            .for_each(|(mesh, (vertices, indices))| {
                mesh.update(&state.device, &state.queue, &vertices, &indices)
            });

        state.bg_mesh.update(
            &state.device,
            &state.queue,
            &self.bg_vertices,
            &self.bg_indices,
        );

        state.overlay_mesh.update(
            &state.device,
            &state.queue,
            &self.overlay_vertices,
            &self.overlay_indices,
        );

        state.model =
            Mat4::from_translation(self.state.position) * Mat4::from_quat(self.state.orientation);
    }

    pub fn draw_padding(&mut self) {
        let tl = -self.state.half_size;
        let br = self.state.half_size;
        let inset = br - self.grid_to_pos(self.grid_size.x as i32, 0);
        let color = self.get_background_color();
        self.draw_hollow_rect(tl, br, inset, color);
    }

    pub fn draw_cell(&mut self, cell: Indexed<&Cell>) {
        if cell.flags.contains(Flags::HIDDEN) {
            return;
        }

        let col = cell.point.column.0 as i32;
        let row = cell.point.line.0;
        let mut fg = cell.fg;
        let mut bg = cell.bg;

        let is_full_block = cell.c == '▀';

        if cell.flags.contains(Flags::INVERSE) ^ is_full_block {
            std::mem::swap(&mut fg, &mut bg);
        }

        let tl = self.grid_to_pos(col, row);
        let br = self.grid_to_pos(col + 1, row + 1);

        let bg = if bg == Color::Named(NamedColor::Background) {
            self.get_background_color()
        } else {
            self.color_to_u32(bg)
        };

        self.draw_solid_rect(tl, br, bg);

        // skip foreground rendering if the entire cell is occupied
        if is_full_block {
            return;
        }

        let style = FontStyle::from_cell_flags(cell.flags);
        let font = self.fonts.get(style);
        let fg = self.color_to_u32(fg);

        let face = font.atlas.face.as_face_ref();
        if let Some(glyph) = face.glyph_index(cell.c) {
            self.glyphs.push((tl, style, glyph.0, fg));
        }

        let baseline = *self.font_baselines.get(style) * self.state.units_per_em;
        let make_line = |pos, width| -> (Vec2, Vec2) {
            let cy = tl.y + pos * self.state.units_per_em - baseline;
            let w = width * self.state.units_per_em;
            let tl = vec2(tl.x, cy);
            let br = vec2(br.x, cy + w);
            (tl, br)
        };

        // pre-calc line variables before mutable borrowing with rect draws
        let so_line = make_line(font.strikeout_pos, font.strikeout_width);
        let ul_line = make_line(font.underline_pos, font.underline_width);

        if cell.flags.contains(Flags::STRIKEOUT) {
            self.draw_solid_rect(so_line.0, so_line.1, fg);
        }

        if cell.flags.contains(Flags::UNDERLINE) {
            self.draw_solid_rect(ul_line.0, ul_line.1, fg);
        }
    }

    pub fn draw_cursor(&mut self, cursor: RenderableCursor) {
        let cursor_color = Color::Named(NamedColor::Foreground);
        let cursor_color = self.color_to_u32(cursor_color);
        let col = cursor.point.column.0 as i32;
        let row = cursor.point.line.0;
        let line_width = 0.1 * self.state.units_per_em;
        match cursor.shape {
            CursorShape::Hidden => {}
            CursorShape::Block => {
                let tl = self.grid_to_pos(col, row);
                let br = self.grid_to_pos(col + 1, row + 1);
                self.draw_solid_rect(tl, br, cursor_color);
            }
            CursorShape::Underline => {
                let tl = self.grid_to_pos(col, row);
                let br = self.grid_to_pos(col + 1, row + 1);
                let tl = vec2(tl.x, br.y + line_width);
                self.draw_solid_rect(tl, br, cursor_color);
            }
            CursorShape::Beam => {
                let tl = self.grid_to_pos(col, row);
                let br = self.grid_to_pos(col + 1, row + 1);
                let br = vec2(tl.x + line_width, br.y);
                self.draw_solid_rect(tl, br, cursor_color);
            }
            CursorShape::HollowBlock => {
                let tl = self.grid_to_pos(col, row);
                let br = self.grid_to_pos(col + 1, row + 1);
                self.draw_hollow_rect(tl, br, Vec2::splat(line_width), cursor_color);
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

    /// `border` can be positive for inset or negative for outset.
    pub fn draw_hollow_rect(&mut self, tl: Vec2, br: Vec2, border: Vec2, color: u32) {
        let bl = vec2(tl.x, br.y); // bottom-left
        let tr = vec2(br.x, tl.y); // top-right
        let bx = Vec2::new(border.x, 0.0); // border-X
        let by = Vec2::new(0.0, border.y); // border-Y

        self.draw_solid_rect(tl, bl + bx, color); // left edge
        self.draw_solid_rect(tr - bx, br, color); // right edge
        self.draw_solid_rect(tl + bx, tr - bx + by, color); // top edge
        self.draw_solid_rect(bl + bx - by, br - bx, color); // bottom edge
    }

    pub fn grid_to_pos(&self, x: i32, y: i32) -> Vec2 {
        let mut pos = IVec2::new(x, y).as_vec2() - self.grid_size.as_vec2() / 2.0;
        pos.y = -pos.y;
        pos * self.cell_size * self.state.units_per_em
    }

    pub fn color_to_rgb(&self, color: Color) -> Rgb {
        match color {
            Color::Named(name) => self.state.colors[name].unwrap_or(Rgb {
                r: 0xff,
                g: 0x00,
                b: 0xff,
            }),
            Color::Spec(rgb) => rgb,
            Color::Indexed(index) => {
                if let Some(color) = self.state.colors[index as usize] {
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

    pub fn get_background_color(&self) -> u32 {
        let bg = Color::Named(NamedColor::Background);
        let base = self.color_to_u32(bg);
        let alpha = (self.state.opacity * 255.0) as u8;
        ((alpha as u32) << 24) | (base & 0x00ffffff)
    }
}
