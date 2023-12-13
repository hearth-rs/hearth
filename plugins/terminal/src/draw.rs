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

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use hearth_rend3::{
    rend3::graph::{
        NodeResourceUsage, RenderGraph, RenderPassDepthTarget, RenderPassTarget, RenderPassTargets,
        RenderTargetHandle,
    },
    utils::DynamicMesh,
    wgpu::*,
};

use crate::text::{FaceAtlas, FontSet};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CameraUniform {
    pub mvp: glam::Mat4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SolidVertex {
    pub position: glam::Vec2,
    pub color: u32,
}

impl SolidVertex {
    pub const LAYOUT: VertexBufferLayout<'static> = VertexBufferLayout {
        array_stride: std::mem::size_of::<Self>() as BufferAddress,
        step_mode: VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                offset: 0,
                format: VertexFormat::Float32x2,
                shader_location: 0,
            },
            VertexAttribute {
                offset: std::mem::size_of::<[f32; 2]>() as BufferAddress,
                format: VertexFormat::Unorm8x4,
                shader_location: 1,
            },
        ],
    };
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct GlyphVertex {
    pub position: glam::Vec2,
    pub tex_coords: glam::Vec2,
    pub color: u32,
}

impl GlyphVertex {
    pub const LAYOUT: VertexBufferLayout<'static> = VertexBufferLayout {
        array_stride: std::mem::size_of::<Self>() as BufferAddress,
        step_mode: VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                offset: 0,
                format: VertexFormat::Float32x2,
                shader_location: 0,
            },
            VertexAttribute {
                offset: std::mem::size_of::<[f32; 2]>() as BufferAddress,
                format: VertexFormat::Float32x2,
                shader_location: 1,
            },
            VertexAttribute {
                offset: std::mem::size_of::<[f32; 4]>() as BufferAddress,
                format: VertexFormat::Unorm8x4,
                shader_location: 2,
            },
        ],
    };
}

/// Common GPU objects used for drawing all terminals.
pub struct TerminalPipelines {
    device: Arc<Device>,
    queue: Arc<Queue>,
    camera_bgl: BindGroupLayout,
    glyph_bgl: BindGroupLayout,
    solid_pipeline: RenderPipeline,
    glyph_pipeline: RenderPipeline,
    atlas_sampler: Sampler,
}

impl TerminalPipelines {
    /// Initialize a device and queue's GPU state targeting the given output
    /// surface format.
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(include_wgsl!("shaders.wgsl"));

        let camera_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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

        let glyph_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("AlacrittyRoutine pipeline layout"),
            bind_group_layouts: &[&camera_bgl, &glyph_bgl],
            push_constant_ranges: &[],
        });

        let make_pipeline = |label, vs, fs, vert_layout| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
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
                    targets: &[Some(ColorTargetState {
                        format,
                        blend: Some(BlendState::ALPHA_BLENDING),
                        write_mask: ColorWrites::COLOR,
                    })],
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

        let atlas_sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            device,
            queue,
            camera_bgl,
            glyph_bgl,
            solid_pipeline,
            glyph_pipeline,
            atlas_sampler,
        }
    }

    /// Adds a set of pipelines and associated set of [TerminalDrawState] to a
    /// rend3 render graph.
    pub fn add_to_graph<'a>(
        &'a self,
        draws: &'a [&'a TerminalDrawState],
        graph: &mut RenderGraph<'a>,
        output: RenderTargetHandle,
        depth: RenderTargetHandle,
    ) {
        let mut builder = graph.add_node("terminal");
        let output_handle = builder.add_render_target(output, NodeResourceUsage::InputOutput);
        let depth_handle = builder.add_render_target(depth, NodeResourceUsage::Input);

        let rpass_handle = builder.add_renderpass(RenderPassTargets {
            targets: vec![RenderPassTarget {
                color: output_handle,
                clear: Color::BLACK,
                resolve: None,
            }],
            depth_stencil: Some(RenderPassDepthTarget {
                target: depth_handle,
                depth_clear: Some(0.0),
                stencil_clear: None,
            }),
        });

        builder.build(move |mut ctx| {
            let rpass = ctx.encoder_or_pass.take_rpass(rpass_handle);
            let vp = ctx.data_core.camera_manager.view_proj();

            for draw in draws {
                self.draw_terminal(draw, rpass, vp);
            }
        });
    }

    /// Renders a single [TerminalDrawState].
    pub fn draw_terminal<'a>(
        &'a self,
        terminal: &'a TerminalDrawState,
        rpass: &mut RenderPass<'a>,
        vp: Mat4,
    ) {
        let model = terminal.model;

        self.queue.write_buffer(
            &terminal.camera_buffer,
            0,
            bytemuck::cast_slice(&[CameraUniform { mvp: vp * model }]),
        );

        // set the camera bind group for all draw calls
        rpass.set_bind_group(0, &terminal.camera_bind_group, &[]);

        // set the regular glyph bind group for solid geo drawing
        rpass.set_bind_group(1, &terminal.glyph_bind_groups.regular, &[]);

        // draw solid geo
        rpass.set_pipeline(&self.solid_pipeline);
        terminal.bg_mesh.draw(rpass);

        // set the glyph pipeline for all glyph draws
        rpass.set_pipeline(&self.glyph_pipeline);

        // draw regular glyphs
        // regular glyph bind group is already bound
        terminal.glyph_meshes.regular.draw(rpass);

        // draw italic glyphs
        rpass.set_bind_group(1, &terminal.glyph_bind_groups.italic, &[]);
        terminal.glyph_meshes.italic.draw(rpass);

        // draw bold glyphs
        rpass.set_bind_group(1, &terminal.glyph_bind_groups.bold, &[]);
        terminal.glyph_meshes.bold.draw(rpass);

        // draw bold italic glyphs
        rpass.set_bind_group(1, &terminal.glyph_bind_groups.bold_italic, &[]);
        terminal.glyph_meshes.bold_italic.draw(rpass);

        // draw overlay geo
        rpass.set_pipeline(&self.solid_pipeline);
        terminal.overlay_mesh.draw(rpass);
    }
}

/// A ready-to-render terminal state.
pub struct TerminalDrawState {
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub model: Mat4,
    pub camera_buffer: Buffer,
    pub camera_bind_group: BindGroup,
    pub glyph_bind_groups: FontSet<BindGroup>,
    pub bg_mesh: DynamicMesh<SolidVertex>,
    pub glyph_meshes: FontSet<DynamicMesh<GlyphVertex>>,
    pub overlay_mesh: DynamicMesh<SolidVertex>,
}

impl TerminalDrawState {
    pub fn new(pipelines: &TerminalPipelines, fonts: FontSet<Arc<FaceAtlas>>) -> Self {
        let device = pipelines.device.as_ref();

        let camera_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Alacritty terminal camera buffer"),
            size: std::mem::size_of::<CameraUniform>() as BufferAddress,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Alacritty terminal camera bind group"),
            layout: &pipelines.camera_bgl,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let glyph_bind_groups = fonts.map(|font| {
            let atlas_view = font.texture.create_view(&Default::default());

            let glyph_bind_group = pipelines.device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &pipelines.glyph_bgl,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&atlas_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&pipelines.atlas_sampler),
                    },
                ],
            });

            glyph_bind_group
        });

        let glyph_meshes = FontSet {
            regular: "Alacritty regular glyph mesh",
            italic: "Alacritty italic glyph mesh",
            bold: "Alacritty bold glyph mesh",
            bold_italic: "Alacritty bold italic glyph mesh",
        }
        .map(|name| DynamicMesh::new(device, Some(name.to_string())));

        Self {
            model: Mat4::IDENTITY,
            camera_buffer,
            camera_bind_group,
            bg_mesh: DynamicMesh::new(device, Some("Alacritty background mesh".into())),
            glyph_meshes,
            overlay_mesh: DynamicMesh::new(device, Some("Alacritty overlay mesh".into())),
            glyph_bind_groups,
            device: pipelines.device.to_owned(),
            queue: pipelines.queue.to_owned(),
        }
    }
}
