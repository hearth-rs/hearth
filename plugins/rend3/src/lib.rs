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

use glam::{UVec2, Vec4};
use hearth_runtime::runtime::{Plugin, RuntimeBuilder};
use rend3::graph::{InstructionEvaluationOutput, RenderGraph, RenderTargetHandle};
use rend3::types::{Camera, SampleCount, TextureCubeHandle};
use rend3::{InstanceAdapterDevice, Renderer, ShaderPreProcessor};
use rend3_routine::base::{BaseRenderGraph, BaseRenderGraphIntermediateState};
use rend3_routine::pbr::PbrRoutine;
use rend3_routine::skybox::SkyboxRoutine;
use rend3_routine::tonemapping::TonemappingRoutine;
use tokio::sync::{mpsc, oneshot};
use wgpu::{SurfaceTexture, TextureFormat};

pub use rend3;
pub use rend3_routine;
pub use wgpu;

pub mod utils;

/// The info about a frame passed to [Routine::draw].
pub struct RoutineInfo<'a, 'graph> {
    pub state: &'a BaseRenderGraphIntermediateState,
    pub sample_count: SampleCount,
    pub resolution: UVec2,
    pub output_handle: RenderTargetHandle,
    pub eval_output: &'a InstructionEvaluationOutput,
    pub graph: &'a mut RenderGraph<'graph>,
}

pub trait Routine: Send + Sync + 'static {
    fn build_node(&mut self) -> Box<dyn Node<'_> + '_>;
}

pub trait Node<'a> {
    fn draw<'graph>(&'graph self, info: &mut RoutineInfo<'_, 'graph>);
}

/// A request to the renderer to draw a single frame.
pub struct FrameRequest {
    /// The output texture.
    pub output_texture: SurfaceTexture,

    /// The dimensions of the frame.
    pub resolution: glam::UVec2,

    /// The camera to use for this frame.
    pub camera: Camera,

    /// This oneshot message is sent when the frame is done rendering.
    pub on_complete: oneshot::Sender<()>,
}

/// An update to the global rend3 state.
pub enum Rend3Command {
    /// Updates the skybox.
    SetSkybox(TextureCubeHandle),

    /// Updates the ambient lighting.
    SetAmbient(Vec4),
}

/// A rend3 Hearth plugin for adding 3D rendering to a Hearth runtime.
///
/// This plugin can be acquired by other plugins during runtime building to add
/// more nodes to the render graph.
pub struct Rend3Plugin {
    pub iad: InstanceAdapterDevice,
    pub surface_format: TextureFormat,
    pub renderer: Arc<Renderer>,
    pub spp: ShaderPreProcessor,
    pub base_render_graph: BaseRenderGraph,
    pub pbr_routine: PbrRoutine,
    pub tonemapping_routine: TonemappingRoutine,
    pub skybox_routine: SkyboxRoutine,
    pub ambient: Vec4,
    pub frame_request_tx: mpsc::UnboundedSender<FrameRequest>,
    pub command_tx: mpsc::UnboundedSender<Rend3Command>,
    new_skybox: Option<TextureCubeHandle>,
    frame_request_rx: mpsc::UnboundedReceiver<FrameRequest>,
    command_rx: mpsc::UnboundedReceiver<Rend3Command>,
    routines: Vec<Box<dyn Routine>>,
}

impl Plugin for Rend3Plugin {
    fn finalize(mut self, _builder: &mut RuntimeBuilder) {
        tokio::spawn(async move {
            while let Some(frame) = self.frame_request_rx.recv().await {
                self.flush_commands();
                self.draw(frame);
            }
        });
    }
}

impl Rend3Plugin {
    /// Creates a new rend3 plugin from an existing [InstanceAdapterDevice] and
    /// the target window's texture format.
    pub fn new(iad: InstanceAdapterDevice, surface_format: TextureFormat) -> Self {
        let handedness = rend3::types::Handedness::Right;
        let renderer = Renderer::new(iad.to_owned(), handedness, None).unwrap();
        let mut spp = ShaderPreProcessor::new();
        rend3_routine::builtin_shaders(&mut spp);
        let base_render_graph = BaseRenderGraph::new(&renderer, &spp);
        let mut data_core = renderer.data_core.lock();
        let interfaces = &base_render_graph.interfaces;
        let culling_buffer = &base_render_graph.gpu_culler.culling_buffer_map_handle;
        let pbr_routine =
            PbrRoutine::new(&renderer, &mut data_core, &spp, interfaces, culling_buffer);
        let tonemapping_routine =
            TonemappingRoutine::new(&renderer, &spp, interfaces, surface_format);
        let skybox_routine = SkyboxRoutine::new(&renderer, &spp, interfaces);
        drop(data_core);

        let (frame_request_tx, frame_request_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        Self {
            iad,
            surface_format,
            renderer,
            spp,
            base_render_graph,
            pbr_routine,
            tonemapping_routine,
            skybox_routine,
            frame_request_tx,
            frame_request_rx,
            command_tx,
            command_rx,
            new_skybox: None,
            ambient: Vec4::ZERO,
            routines: Vec::new(),
        }
    }

    /// Adds a new [Routine] to this plugin.
    pub fn add_routine(&mut self, routine: impl Routine) {
        self.routines.push(Box::new(routine));
    }

    /// Flushes and applies all [Rend3Command] messages.
    pub fn flush_commands(&mut self) {
        while let Ok(command) = self.command_rx.try_recv() {
            use Rend3Command::*;
            match command {
                SetSkybox(texture) => {
                    self.new_skybox = Some(texture);
                }
                SetAmbient(ambient) => {
                    self.ambient = ambient;
                }
            }
        }
    }

    /// Draws a frame in response to a [FrameRequest].
    pub fn draw(&mut self, request: FrameRequest) {
        self.renderer.swap_instruction_buffers();
        let mut eval_output = self.renderer.evaluate_instructions();

        if let Some(skybox) = self.new_skybox.take() {
            self.skybox_routine.set_background_texture(Some(skybox));
            self.skybox_routine.evaluate(&self.renderer);
        }

        let aspect = request.resolution.as_vec2();
        let aspect = aspect.x / aspect.y;
        self.renderer.set_aspect_ratio(aspect);
        self.renderer.set_camera_data(request.camera);

        let nodes: Vec<_> = self
            .routines
            .iter_mut()
            .map(|routine| routine.build_node())
            .collect();

        let mut graph_data = RenderGraph::new();
        let graph = &mut graph_data;
        let samples = SampleCount::One;
        let base = &self.base_render_graph;
        let resolution = request.resolution;
        let ambient = self.ambient;
        let pbr = &self.pbr_routine;
        let skybox = Some(&self.skybox_routine);
        let clear_color = Vec4::ZERO;
        let tonemapping = &self.tonemapping_routine;

        // add output frame
        let output_handle = graph.add_imported_render_target(
            &request.output_texture.texture,
            0..1,
            0..1,
            rend3::graph::ViewportRect {
                offset: UVec2::ZERO,
                size: resolution,
            },
        );

        // see implementation of BaseRenderGraph::add_to_graph() for details
        // on what the following code is based on
        //
        // we need to override this function so that we can hook into the
        // graph's state in our custom nodes

        // Create the data and handles for the graph.
        let state = BaseRenderGraphIntermediateState::new(graph, &eval_output, resolution, samples);

        // Clear the shadow map.
        state.clear_shadow(graph);

        // Prepare all the uniforms that all shaders need access to.
        state.create_frame_uniforms(graph, base, ambient, resolution);

        // Perform compute based skinning.
        state.skinning(graph, base);

        // Upload the uniforms for the objects in the shadow pass.
        state.shadow_object_uniform_upload(graph, base, &eval_output);
        // Perform culling for the objects in the shadow pass.
        state.pbr_shadow_culling(graph, base);

        // Render all the shadows to the shadow map.
        state.pbr_shadow_rendering(graph, pbr, &eval_output.shadows);

        // Clear the primary render target and depth target.
        state.clear(graph, clear_color);

        // Upload the uniforms for the objects in the forward pass.
        state.object_uniform_upload(graph, base, resolution, samples);

        // Do the first pass, rendering the predicted triangles from last frame.
        state.pbr_render_opaque_predicted_triangles(graph, pbr, samples);

        // Create the hi-z buffer.
        state.hi_z(graph, pbr, resolution);

        // Perform culling for the objects in the forward pass.
        //
        // The result of culling will be used to predict the visible triangles for
        // the next frame. It will also render all the triangles that were visible
        // but were not predicted last frame.
        state.pbr_culling(graph, base);

        // Do the second pass, rendering the residual triangles.
        state.pbr_render_opaque_residual_triangles(graph, pbr, samples);

        // Render the skybox.
        state.skybox(graph, skybox, samples);

        // Render all transparent objects.
        //
        // This _must_ happen after culling, as all transparent objects are
        // considered "residual".
        state.pbr_forward_rendering_transparent(graph, pbr, samples);

        // Tonemap the HDR inner buffer to the output buffer.
        state.tonemapping(graph, tonemapping, output_handle);

        let mut info = RoutineInfo {
            state: &state,
            sample_count: SampleCount::One,
            resolution: request.resolution,
            output_handle,
            eval_output: &eval_output,
            graph,
        };

        for node in nodes.iter() {
            node.draw(&mut info);
        }

        graph_data.execute(&self.renderer, &mut eval_output);

        request.output_texture.present();

        let _ = request.on_complete.send(()); // ignore hangup
    }
}
