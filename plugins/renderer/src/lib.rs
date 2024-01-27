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

use hearth_rend3::{
    rend3::{types::*, *},
    rend3_routine::pbr::{AlbedoComponent, PbrMaterial},
    Rend3Command, Rend3Plugin,
};
use hearth_runtime::{
    anyhow::{self, bail},
    asset::{AssetLoader, AssetStore, JsonAssetLoader},
    async_trait,
    hearth_macros::GetProcessMetadata,
    hearth_schema::{renderer::*, LumpId},
    runtime::{Plugin, RuntimeBuilder},
    tokio::sync::mpsc::UnboundedSender,
    tracing::{error, warn},
    utils::*,
};

pub struct MeshLoader(Arc<Renderer>);

#[async_trait]
impl JsonAssetLoader for MeshLoader {
    type Asset = MeshHandle;
    type Data = MeshData;

    async fn load_asset(
        &self,
        _store: &AssetStore,
        data: Self::Data,
    ) -> anyhow::Result<Self::Asset> {
        let mesh = Mesh {
            vertex_positions: data.positions.0,
            vertex_normals: data.normals.0,
            vertex_tangents: data.tangents.0,
            vertex_uv0: data.uv0.0,
            vertex_uv1: data.uv1.0,
            vertex_colors: data.colors.0,
            vertex_joint_indices: data.joint_indices.0,
            vertex_joint_weights: data.joint_weights.0,
            indices: data.indices.0,
        };

        let _ = mesh.validate()?;

        let handle = self.0.add_mesh(mesh);

        Ok(handle)
    }
}

pub struct MaterialLoader(Arc<Renderer>);

#[async_trait]
impl JsonAssetLoader for MaterialLoader {
    type Asset = MaterialHandle;
    type Data = MaterialData;

    async fn load_asset(
        &self,
        store: &AssetStore,
        data: Self::Data,
    ) -> anyhow::Result<Self::Asset> {
        let albedo = store.load_asset::<TextureLoader>(&data.albedo).await?;

        let material = PbrMaterial {
            albedo: AlbedoComponent::Texture(albedo.as_ref().to_owned()),
            ..Default::default()
        };

        let handle = self.0.add_material(material);
        Ok(handle)
    }
}

pub struct TextureLoader(Arc<Renderer>);

#[async_trait]
impl JsonAssetLoader for TextureLoader {
    type Asset = TextureHandle;
    type Data = TextureData;

    async fn load_asset(
        &self,
        _store: &AssetStore,
        data: Self::Data,
    ) -> anyhow::Result<Self::Asset> {
        let expected_len = (data.size.x * data.size.y * 4) as usize;

        if data.data.len() != expected_len {
            bail!("invalid texture data length");
        }

        let texture = Texture {
            label: data.label,
            data: data.data,
            format: TextureFormat::Rgba8UnormSrgb,
            size: data.size,
            mip_count: MipmapCount::ONE,
            mip_source: MipmapSource::Uploaded,
        };

        let handle = self.0.add_texture_2d(texture);
        Ok(handle)
    }
}

pub struct CubeTextureLoader(Arc<Renderer>);

#[async_trait]
impl JsonAssetLoader for CubeTextureLoader {
    type Asset = TextureHandle;
    type Data = TextureData;

    async fn load_asset(
        &self,
        _store: &AssetStore,
        data: Self::Data,
    ) -> anyhow::Result<Self::Asset> {
        let expected_len = (data.size.x * data.size.y * 24) as usize;

        if data.data.len() != expected_len {
            bail!("invalid texture data length");
        }

        let texture = Texture {
            label: data.label,
            data: data.data,
            format: TextureFormat::Rgba8UnormSrgb,
            size: data.size,
            mip_count: MipmapCount::ONE,
            mip_source: MipmapSource::Generated,
        };

        let handle = self.0.add_texture_cube(texture);

        Ok(handle)
    }
}

/// An instance of a renderer directional light. Accepts DirectionalLightUpdate.
#[derive(GetProcessMetadata)]
pub struct DirectionalLightInstance {
    renderer: Arc<Renderer>,
    handle: ResourceHandle<DirectionalLight>,
}

#[async_trait]
impl SinkProcess for DirectionalLightInstance {
    type Message = DirectionalLightUpdate;

    async fn on_message<'a>(&'a mut self, message: MessageInfo<'a, Self::Message>) {
        let mut change = DirectionalLightChange::default();

        use DirectionalLightUpdate::*;
        match message.data {
            Color(color) => change.color = Some(color),
            Intensity(intensity) => change.intensity = Some(intensity),
            Direction(direction) => change.direction = Some(direction),
            Distance(distance) => change.distance = Some(distance),
        }

        self.renderer.update_directional_light(&self.handle, change);
    }
}

/// An instance of a renderer object. Accepts ObjectUpdate.
#[derive(GetProcessMetadata)]
pub struct ObjectInstance {
    renderer: Arc<Renderer>,
    handle: ObjectHandle,
    skeleton: Option<SkeletonHandle>,
}

#[async_trait]
impl SinkProcess for ObjectInstance {
    type Message = ObjectUpdate;

    async fn on_message<'a>(&'a mut self, message: MessageInfo<'a, Self::Message>) {
        use ObjectUpdate::*;
        match &message.data {
            Transform(transform) => {
                self.renderer.set_object_transform(&self.handle, *transform);
            }
            JointMatrices(matrices) => {
                let Some(skeleton) = self.skeleton.as_ref() else {
                    warn!("tried to update joint matrices on static object");
                    return;
                };

                self.renderer
                    .set_skeleton_joint_matrices(skeleton, matrices.to_owned());
            }
            JointTransforms {
                joint_global,
                inverse_bind,
            } => {
                let Some(skeleton) = self.skeleton.as_ref() else {
                    warn!("tried to update joint transforms on static object");
                    return;
                };

                self.renderer
                    .set_skeleton_joint_transforms(skeleton, joint_global, inverse_bind);
            }
        }
    }
}

/// The native interface to the renderer. Accepts RendererRequest.
#[derive(GetProcessMetadata)]
pub struct RendererService {
    renderer: Arc<Renderer>,
    command_tx: UnboundedSender<Rend3Command>,
}

#[async_trait]
impl RequestResponseProcess for RendererService {
    type Request = RendererRequest;
    type Response = RendererResponse;

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, Self::Request>,
    ) -> ResponseInfo<'a, Self::Response> {
        use RendererRequest::*;
        match &request.data {
            AddDirectionalLight { initial_state } => {
                let light = DirectionalLight {
                    color: initial_state.color,
                    intensity: initial_state.intensity,
                    direction: initial_state.direction,
                    distance: initial_state.distance,
                };

                let handle = self.renderer.add_directional_light(light);

                let child = request.spawn(DirectionalLightInstance {
                    renderer: self.renderer.clone(),
                    handle,
                });

                return ResponseInfo {
                    data: Ok(RendererSuccess::Ok),
                    caps: vec![child],
                };
            }
            AddObject {
                mesh,
                skeleton,
                material,
                transform,
            } => {
                let mesh = match Self::try_load_asset::<MeshLoader>(&request, mesh).await {
                    Ok(mesh) => mesh,
                    Err(err) => return err.into(),
                };

                let material =
                    match Self::try_load_asset::<MaterialLoader>(&request, material).await {
                        Ok(material) => material,
                        Err(err) => return err.into(),
                    };

                let (mesh_kind, skeleton) = if let Some(skeleton) = skeleton.as_ref() {
                    let skeleton = self.renderer.add_skeleton(Skeleton {
                        joint_matrices: skeleton.to_owned(),
                        mesh: mesh.as_ref().to_owned(),
                    });

                    (ObjectMeshKind::Animated(skeleton.clone()), Some(skeleton))
                } else {
                    (ObjectMeshKind::Static(mesh.as_ref().to_owned()), None)
                };

                let object = Object {
                    mesh_kind,
                    material: material.as_ref().to_owned(),
                    transform: *transform,
                };

                let handle = self.renderer.add_object(object);

                let child = request.spawn(ObjectInstance {
                    renderer: self.renderer.clone(),
                    handle,
                    skeleton,
                });

                return ResponseInfo {
                    data: Ok(RendererSuccess::Ok),
                    caps: vec![child],
                };
            }
            SetSkybox { texture } => {
                let texture =
                    match Self::try_load_asset::<CubeTextureLoader>(&request, texture).await {
                        Ok(texture) => texture,
                        Err(err) => return err.into(),
                    };

                let _ = self
                    .command_tx
                    .send(Rend3Command::SetSkybox(texture.as_ref().clone()));
            }
            SetAmbientLighting { ambient } => {
                let _ = self.command_tx.send(Rend3Command::SetAmbient(*ambient));
            }
        }

        ResponseInfo {
            data: Ok(RendererSuccess::Ok),
            caps: vec![],
        }
    }
}

impl ServiceRunner for RendererService {
    const NAME: &'static str = "hearth.Renderer";
}

impl RendererService {
    pub fn new(renderer: Arc<Renderer>, command_tx: UnboundedSender<Rend3Command>) -> Self {
        Self {
            renderer,
            command_tx,
        }
    }

    /// Helper function to attempt to load an asset but log a warning and return
    /// a `RendererError::LumpError` if unsuccessful.
    async fn try_load_asset<T: AssetLoader>(
        request: &RequestInfo<'_, RendererRequest>,
        lump: &LumpId,
    ) -> Result<Arc<T::Asset>, RendererError> {
        request
            .runtime
            .asset_store
            .load_asset::<T>(lump)
            .await
            .map_err(|err| {
                error!(
                    "failed to load {}: {err:?}",
                    std::any::type_name::<T::Asset>(),
                );

                RendererError::LumpError
            })
    }
}

/// Initializes guest-available rendering code.
#[derive(Default)]
pub struct RendererPlugin {}

impl Plugin for RendererPlugin {
    fn build(&mut self, builder: &mut RuntimeBuilder) {
        let rend3 = builder
            .get_plugin::<Rend3Plugin>()
            .expect("rend3 plugin was not found");

        let renderer = rend3.renderer.clone();
        let command_tx = rend3.command_tx.clone();

        builder
            .add_asset_loader(MeshLoader(renderer.clone()))
            .add_asset_loader(MaterialLoader(renderer.clone()))
            .add_asset_loader(TextureLoader(renderer.clone()))
            .add_asset_loader(CubeTextureLoader(renderer.clone()))
            .add_plugin(RendererService::new(renderer, command_tx));
    }
}
