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

use std::sync::Arc;

use rend3::graph::{
    RenderGraph, RenderPassDepthTarget, RenderPassTarget, RenderPassTargets, RenderTargetHandle,
};
use rend3::Renderer;

use terminal::TerminalDrawState;
use wgpu::*;

pub use alacritty_terminal;
pub use owned_ttf_parser;

/// Terminal-related logic and helpers.
pub mod terminal;

/// Text management.
pub mod text;

/// GPU-side logic and helpers.
pub mod gpu;

use crate::{
    gpu::{CameraUniform, GlyphVertex, SolidVertex},
    terminal::{Terminal, TerminalConfig},
    text::FontSet,
};

pub struct TerminalWrapper {
    terminal: Arc<Terminal>,
    draw_state: TerminalDrawState,
}

impl TerminalWrapper {
    /// Updates this terminal's draw state. Returns true if this terminal has not quit.
    pub fn update(&mut self) -> bool {
        let quit = self.terminal.should_quit();

        if !quit {
            self.terminal.update_draw_state(&mut self.draw_state);
        }

        !quit
    }
}

/// Persistent terminal rendering configuration and handles to active terminals.
pub struct TerminalStore {
    device: Arc<Device>,
    queue: Arc<Queue>,
    camera_bgl: BindGroupLayout,
    glyph_bind_groups: FontSet<BindGroup>,
    solid_pipeline: RenderPipeline,
    glyph_pipeline: RenderPipeline,
    terminals: Vec<TerminalWrapper>,
}

impl TerminalStore {
    /// This routine runs after tonemapping, so `format` is the format of the
    /// final swapchain image format.
    pub fn new(config: TerminalConfig, renderer: &Renderer, format: TextureFormat) -> Self {
        let shader = renderer
            .device
            .create_shader_module(&include_wgsl!("shaders.wgsl"));

        let camera_bgl = renderer
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Alacritty camera bind group layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let glyph_bgl = renderer
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Alacritty glyph bind group layout"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            multisampled: false,
                            view_dimension: TextureViewDimension::D2,
                            sample_type: TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let layout = renderer
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("AlacrittyRoutine pipeline layout"),
                bind_group_layouts: &[&camera_bgl, &glyph_bgl],
                push_constant_ranges: &[],
            });

        let make_pipeline = |label, vs, fs, vert_layout| {
            renderer
                .device
                .create_render_pipeline(&RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    vertex: VertexState {
                        module: &shader,
                        entry_point: vs,
                        buffers: &[vert_layout],
                    },
                    depth_stencil: Some(DepthStencilState {
                        format: TextureFormat::Depth32Float,
                        depth_write_enabled: false,
                        depth_compare: CompareFunction::GreaterEqual,
                        stencil: StencilState::default(),
                        bias: DepthBiasState::default(),
                    }),
                    primitive: PrimitiveState {
                        topology: PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: FrontFace::Ccw,
                        cull_mode: None,
                        unclipped_depth: false,
                        polygon_mode: PolygonMode::Fill,
                        conservative: false,
                    },
                    multisample: MultisampleState::default(),
                    fragment: Some(FragmentState {
                        module: &shader,
                        entry_point: fs,
                        targets: &[ColorTargetState {
                            format,
                            blend: Some(BlendState::ALPHA_BLENDING),
                            write_mask: ColorWrites::COLOR,
                        }],
                    }),
                    multiview: None,
                })
        };

        let solid_pipeline = make_pipeline(
            "AlacrittyRoutine solid pipeline",
            "solid_vs",
            "solid_fs",
            SolidVertex::LAYOUT,
        );

        let glyph_pipeline = make_pipeline(
            "AlacrittyRoutine glyph pipeline",
            "glyph_vs",
            "glyph_fs",
            GlyphVertex::LAYOUT,
        );

        let atlas_sampler = renderer.device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let glyph_bind_groups = config.fonts.as_ref().map(|font| {
            let atlas_view = font.texture.create_view(&Default::default());

            let glyph_bind_group = renderer.device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &glyph_bgl,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&atlas_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&atlas_sampler),
                    },
                ],
            });

            glyph_bind_group
        });

        Self {
            device: renderer.device.to_owned(),
            queue: renderer.queue.to_owned(),
            camera_bgl,
            glyph_bind_groups,
            solid_pipeline,
            glyph_pipeline,
            terminals: vec![],
        }
    }

    /// Inserts a new terminal into this store.
    pub fn insert_terminal(&mut self, terminal: &Arc<Terminal>) {
        self.terminals.push(TerminalWrapper {
            terminal: terminal.to_owned(),
            draw_state: TerminalDrawState::new(
                self.device.to_owned(),
                self.queue.to_owned(),
                &self.camera_bgl,
            ),
        });
    }

    /// Creates a new render routine for this frame.
    ///
    /// Note that this locks the mutexes on all owned terminals for as long as
    /// this routine is owned. Create and then drop as quickly as possible!
    pub fn create_routine(&mut self) -> TerminalRenderRoutine<'_> {
        self.terminals.retain_mut(TerminalWrapper::update);

        let terminals: Vec<_> = self.terminals.iter().map(|t| &t.draw_state).collect();

        TerminalRenderRoutine {
            store: self,
            terminals,
        }
    }

    /// Draws a single terminal to a render pass.
    pub fn draw_terminal<'a>(
        &'a self,
        rpass: &mut RenderPass<'a>,
        terminal: &'a TerminalDrawState,
        vp: glam::Mat4,
    ) {
        let model = terminal.model;

        self.queue.write_buffer(
            &terminal.camera_buffer,
            0,
            bytemuck::cast_slice(&[CameraUniform { mvp: vp * model }]),
        );

        rpass.set_bind_group(0, &terminal.camera_bind_group, &[]);
        rpass.set_bind_group(1, &self.glyph_bind_groups.regular, &[]);
        rpass.set_pipeline(&self.solid_pipeline);
        terminal.bg_mesh.draw(rpass);
        rpass.set_pipeline(&self.glyph_pipeline);
        terminal.glyph_meshes.regular.draw(rpass);
        rpass.set_bind_group(1, &self.glyph_bind_groups.italic, &[]);
        terminal.glyph_meshes.italic.draw(rpass);
        rpass.set_bind_group(1, &self.glyph_bind_groups.bold, &[]);
        terminal.glyph_meshes.bold.draw(rpass);
        rpass.set_bind_group(1, &self.glyph_bind_groups.bold_italic, &[]);
        terminal.glyph_meshes.bold_italic.draw(rpass);
        rpass.set_pipeline(&self.solid_pipeline);
        terminal.overlay_mesh.draw(rpass);
    }
}

pub struct TerminalRenderRoutine<'a> {
    store: &'a TerminalStore,
    terminals: Vec<&'a TerminalDrawState>,
}

impl<'a> TerminalRenderRoutine<'a> {
    pub fn add_to_graph<'node>(
        &'node self,
        graph: &mut RenderGraph<'node>,
        output: RenderTargetHandle,
        depth: RenderTargetHandle,
    ) {
        let mut builder = graph.add_node("alacritty");
        let output_handle = builder.add_render_target_output(output);
        let depth_handle = builder.add_render_target_output(depth);
        let rpass_handle = builder.add_renderpass(RenderPassTargets {
            targets: vec![RenderPassTarget {
                color: output_handle,
                clear: rend3::types::Color::BLACK,
                resolve: None,
            }],
            depth_stencil: Some(RenderPassDepthTarget {
                target: rend3::graph::DepthHandle::RenderTarget(depth_handle),
                depth_clear: Some(0.0),
                stencil_clear: None,
            }),
        });

        let store = builder.passthrough_ref(self.store);

        let terminals: Vec<_> = self
            .terminals
            .iter()
            .map(|t| builder.passthrough_ref(*t))
            .collect();

        builder.build(
            move |pt, _renderer, encoder_or_pass, _temps, _ready, graph_data| {
                let store = pt.get(store);
                let rpass = encoder_or_pass.get_rpass(rpass_handle);
                let vp = graph_data.camera_manager.view_proj();

                for terminal in terminals {
                    store.draw_terminal(rpass, pt.get(terminal), vp);
                }
            },
        );
    }
}
