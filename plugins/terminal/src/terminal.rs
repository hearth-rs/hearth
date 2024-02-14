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
use glam::{vec2, IVec2, Mat4, UVec2, Vec2};
use hearth_rend3::wgpu::{Extent3d, ImageCopyTexture, ImageDataLayout, Origin3d, TextureAspect};
use hearth_schema::terminal::TerminalState;
use mio_extras::channel::Sender as MioSender;
use owned_ttf_parser::AsFaceRef;

use crate::{
    draw::{GlyphVertex, SolidVertex, TerminalDrawState, TerminalPipelines},
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
            .floor()
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

    pub fn get_fonts(&self) -> FontSet<Arc<FaceAtlas>> {
        self.fonts.as_ref().map(|font| font.atlas.to_owned())
    }

    pub fn update(&self, state: TerminalState) {
        let mut inner = self.inner.lock();

        let available = (state.half_size - state.padding) * 2.0;
        let grid_size = (available / self.cell_size / state.units_per_em)
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

    pub fn update_draw_state(&self, pipelines: &TerminalPipelines, draw: &mut TerminalDrawState) {
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

        canvas.apply_to_state(pipelines, draw);
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
                let color = self
                    .inner
                    .lock()
                    .state
                    .colors
                    .get(&index)
                    .map(|color| {
                        let (_, r, g, b) = color.to_argb();
                        Rgb { r, g, b }
                    })
                    .unwrap_or(Rgb {
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

/// An in-progress terminal draw state.
pub struct TerminalCanvas {
    fonts: FontSet<FaceWithMetrics>,
    bg_texture: Vec<u32>,
    overlay_vertices: Vec<SolidVertex>,
    overlay_indices: Vec<u32>,
    glyphs: Vec<(Vec2, FontStyle, u16, u32)>,
    state: TerminalState,
    colors: Colors,
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
        let mut colors = Colors::default();

        for (index, color) in state.colors.iter() {
            let (_a, r, g, b) = color.to_argb();
            colors[*index] = Some(Rgb { r, g, b });
        }

        Self {
            fonts,
            bg_texture: vec![0; (grid_size.x * grid_size.y) as usize],
            overlay_vertices: Vec::new(),
            overlay_indices: Vec::new(),
            glyphs: Vec::new(),
            state,
            colors,
            grid_size,
            cell_size,
            font_baselines,
        }
    }

    pub fn update_from_content(&mut self, content: RenderableContent) {
        self.draw_padding();

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

    pub fn apply_to_state(&self, pipelines: &TerminalPipelines, state: &mut TerminalDrawState) {
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

        state.grid_half_size = self.grid_to_pos(self.grid_size.x as i32, self.grid_size.y as i32);

        state.grid_size = self.grid_size;

        if state.grid_capacity.x < state.grid_size.x || state.grid_capacity.y < state.grid_size.y {
            let (texture, bind_group) =
                TerminalDrawState::make_grid(pipelines, &state.grid_buffer, state.grid_size);

            state.grid_texture = texture;
            state.grid_bind_group = bind_group;
            state.grid_capacity = state.grid_size;
        }

        state.queue.write_texture(
            ImageCopyTexture {
                texture: &state.grid_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(self.bg_texture.as_slice()),
            ImageDataLayout {
                offset: 0,
                bytes_per_row: Some((state.grid_size.x * 4).try_into().unwrap()),
                rows_per_image: Some(state.grid_size.y.try_into().unwrap()),
            },
            Extent3d {
                width: state.grid_size.x,
                height: state.grid_size.y,
                depth_or_array_layers: 1,
            },
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

        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        let tl = self.grid_to_pos(col, row);
        let br = self.grid_to_pos(col + 1, row + 1);

        let bg = if bg == Color::Named(NamedColor::Background) {
            self.get_background_color()
        } else {
            self.color_to_u32(bg)
        };

        let idx = (row * (self.grid_size.x as i32) + col) as usize;
        self.bg_texture[idx] = bg;

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
        let index = self.overlay_vertices.len() as u32;
        self.overlay_vertices.extend_from_slice(&[
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

        self.overlay_indices.extend_from_slice(&[
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
            Color::Named(name) => self.colors[name].unwrap_or(Rgb {
                r: 0xff,
                g: 0x00,
                b: 0xff,
            }),
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

    pub fn get_background_color(&self) -> u32 {
        let bg = Color::Named(NamedColor::Background);
        let base = self.color_to_u32(bg);
        let alpha = (self.state.opacity * 255.0) as u8;
        ((alpha as u32) << 24) | (base & 0x00ffffff)
    }
}
