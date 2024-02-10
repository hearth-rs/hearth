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

use glam::{Mat3, Mat4, UVec2, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

use crate::{ByteVec, LumpId};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RendererRequest {
    /// Adds a new directional light to the scene.
    ///
    /// Returns [RendererSuccess::Ok] and a capability to the new light when
    /// successful. The light accepts [DirectionalLightUpdate] messages.
    ///
    /// When the capability is killed, the light is removed from the scene.
    AddDirectionalLight {
        initial_state: DirectionalLightState,
    },

    /// Adds a new object to the scene.
    ///
    /// Returns [RendererSuccess::Ok] and a capability to the new object when
    /// successful. The object accepts [ObjectUpdate] messages.
    ///
    /// When the capability is killed, the object is removed from the scene.
    AddObject {
        /// The lump ID of the [MeshData] to use for this object.
        mesh: LumpId,

        /// An optional list of skeleton joint matrices for this object.
        skeleton: Option<Vec<Mat4>>,

        /// The lump ID of the [MaterialData] to use for this object.
        material: LumpId,

        /// The initial transform of this object.
        transform: Mat4,
    },

    /// Updates the scene's skybox.
    ///
    /// Returns [RendererSuccess::Ok] with no capabilities when successful.
    SetSkybox {
        /// The lump ID of the cube texture to use for this skybox.
        texture: LumpId,
    },

    /// Updates the scene's ambient lighting.
    ///
    /// Returns [RendererSuccess::Ok] with no capabilities when successful.
    SetAmbientLighting { ambient: Vec4 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RendererSuccess {
    /// The request succeeded.
    ///
    /// Capabilities returned by this response are defined by the request kind.
    Ok,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RendererError {
    /// A lump involved in this operation was improperly formatted or not found.
    LumpError,
}

pub type RendererResponse = Result<RendererSuccess, RendererError>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DirectionalLightState {
    pub color: Vec3,
    pub intensity: f32,
    pub direction: Vec3,
    pub distance: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum DirectionalLightUpdate {
    Color(Vec3),
    Intensity(f32),
    Direction(Vec3),
    Distance(f32),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ObjectUpdate {
    Transform(Mat4),
    JointMatrices(Vec<Mat4>),
    JointTransforms {
        joint_global: Vec<Mat4>,
        inverse_bind: Vec<Mat4>,
    },
}

/// A material lump's data format.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MaterialData {
    pub albedo: AlbedoComponent,
    pub transparency: Transparency,
    pub normal: Option<NormalTexture>,
    pub aomr_textures: AoMRTextures,
    pub ao_factor: Option<f32>,
    pub metallic_factor: Option<f32>,
    pub roughness_factor: Option<f32>,
    pub clearcoat_textures: ClearcoatTextures,
    pub clearcoat_factor: Option<f32>,
    pub clearcoat_roughness_factor: Option<f32>,
    pub emissive: MaterialComponent<Vec3>,
    pub reflectance: MaterialComponent<f32>,
    pub anisotropy: MaterialComponent<f32>,
    pub uv_transform0: Mat3,
    pub uv_transform1: Mat3,
    pub unlit: bool,
    pub sample_type: SampleType,
}

/// How a material's albedo color should be determined.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlbedoComponent {
    /// Albedo color factors in vertex value.
    ///
    /// Inner value enables conversion from srgb -> linear before multiplication.
    pub vertex: Option<bool>,

    /// Albedo factors in a fixed value.
    pub value: Option<Vec4>,

    /// Albedo factor is sampled from a given [TextureData] lump.
    pub texture: Option<LumpId>,
}

/// How transparency should be handled in a material.
#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
pub enum Transparency {
    /// Alpha is completely ignored.
    Opaque,

    /// Pixels with alpha less than `cutout` is discarded.
    Cutout { cutout: f32 },

    /// Alpha is blended.
    Blend,
}

/// How a material's normals should be derived.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NormalTexture {
    /// The [TextureData] of this normal texture.
    pub texture: LumpId,

    /// The direction of the texture's normals.
    pub direction: NormalTextureYDirection,

    /// The texture's components to use for normal mapping.
    pub components: NormalTextureComponents,
}

/// The direction of the Y (i.e. green) value in the normal maps.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum NormalTextureYDirection {
    /// Right handed. X right, Y up. OpenGL convention.
    Up,

    /// Left handed. X right, Y down. DirectX convention.
    Down,
}

/// A normal map's component configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum NormalTextureComponents {
    /// Normal stored in RGB values.
    Tricomponent,

    /// Normal stored in RG values, third value is reconstructed.
    Bicomponent,

    /// Normal stored in green and alpha values, third value is reconstructed.
    ///
    /// Useful for storing in BC3 or BC7 compressed textures.
    BicomponentSwizzled,
}

/// How the Ambient Occlusion, Metalic, and Roughness values should be
/// determined.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum AoMRTextures {
    None,
    Combined {
        /// Texture with Ambient Occlusion in R, Roughness in G, and Metallic in
        /// B
        texture: Option<LumpId>,
    },
    SwizzledSplit {
        /// Texture with Ambient Occlusion in R
        ao_texture: Option<LumpId>,
        /// Texture with Roughness in G and Metallic in B
        mr_texture: Option<LumpId>,
    },
    Split {
        /// Texture with Ambient Occlusion in R
        ao_texture: Option<LumpId>,
        /// Texture with Roughness in R and Metallic in G
        mr_texture: Option<LumpId>,
    },
    BWSplit {
        /// Texture with Ambient Occlusion in R
        ao_texture: Option<LumpId>,
        /// Texture with Metallic in R
        m_texture: Option<LumpId>,
        /// Texture with Roughness in R
        r_texture: Option<LumpId>,
    },
}

/// How material clearcoat values should be derived.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ClearcoatTextures {
    None,
    GltfCombined {
        /// Texture with Clearcoat in R, and Clearcoat Roughness in G
        texture: Option<LumpId>,
    },
    GltfSplit {
        /// Texture with Clearcoat in R
        clearcoat_texture: Option<LumpId>,
        /// Texture with Clearcoat Roughness in G
        clearcoat_roughness_texture: Option<LumpId>,
    },
    BWSplit {
        /// Texture with Clearcoat in R
        clearcoat_texture: Option<LumpId>,
        /// Texture with Clearcoat Roughness in R
        clearcoat_roughness_texture: Option<LumpId>,
    },
}

/// Generic container for a component of a material that could either be from a
/// texture or a fixed value.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MaterialComponent<T> {
    pub value: Option<T>,
    pub texture: Option<LumpId>,
}

/// How textures should be sampled.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum SampleType {
    Nearest,
    Linear,
}

/// A mesh lump's data format.
///
/// All vertex attributes must be the same length.
#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MeshData {
    #[serde_as(as = "Base64")]
    pub positions: ByteVec<Vec3>,

    #[serde_as(as = "Base64")]
    pub normals: ByteVec<Vec3>,

    #[serde_as(as = "Base64")]
    pub tangents: ByteVec<Vec3>,

    #[serde_as(as = "Base64")]
    pub uv0: ByteVec<Vec2>,

    #[serde_as(as = "Base64")]
    pub uv1: ByteVec<Vec2>,

    #[serde_as(as = "Base64")]
    pub colors: ByteVec<[u8; 4]>,

    #[serde_as(as = "Base64")]
    pub joint_indices: ByteVec<[u16; 4]>,

    #[serde_as(as = "Base64")]
    pub joint_weights: ByteVec<Vec4>,

    #[serde_as(as = "Base64")]
    pub indices: ByteVec<u32>,
}

/// A texture lump's data format.
#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TextureData {
    /// An optional label for this texture.
    pub label: Option<String>,

    /// The size of this texture.
    pub size: UVec2,

    /// The data of this texture. Currently only supports RGBA sRGB. Must be
    /// a size equivalent to `size.x * size.y * 4`.
    #[serde_as(as = "Base64")]
    pub data: Vec<u8>,
}
