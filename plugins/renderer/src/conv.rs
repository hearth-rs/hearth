// Copyright (c) 2024 Marceline Cramer
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
    rend3::types::{ResourceHandle, Texture},
    rend3_routine::pbr as rend3,
};
use hearth_runtime::{
    anyhow::Result,
    asset::AssetStore,
    hearth_schema::{renderer as schema, LumpId},
};

use crate::TextureLoader;

pub async fn conv_material(
    store: &AssetStore,
    data: schema::MaterialData,
) -> Result<rend3::PbrMaterial> {
    Ok(rend3::PbrMaterial {
        albedo: conv_albedo(store, &data.albedo).await?,
        transparency: conv_transparency(&data.transparency),
        normal: conv_normal(store, &data.normal).await?,
        aomr_textures: conv_aomr_textures(store, &data.aomr_textures).await?,
        ao_factor: data.ao_factor,
        metallic_factor: data.metallic_factor,
        roughness_factor: data.roughness_factor,
        clearcoat_textures: conv_clearcoat_textures(store, &data.clearcoat_textures).await?,
        clearcoat_factor: data.clearcoat_factor,
        clearcoat_roughness_factor: data.clearcoat_roughness_factor,
        emissive: conv_material_component(store, &data.emissive).await?,
        reflectance: conv_material_component(store, &data.reflectance).await?,
        anisotropy: conv_material_component(store, &data.anisotropy).await?,
        uv_transform0: data.uv_transform0,
        uv_transform1: data.uv_transform1,
        unlit: data.unlit,
        sample_type: conv_sample_type(&data.sample_type),
    })
}

pub async fn conv_albedo(
    store: &AssetStore,
    data: &schema::AlbedoComponent,
) -> Result<rend3::AlbedoComponent> {
    let texture = if let Some(lump) = data.texture {
        Some(load_texture(store, lump).await?)
    } else {
        None
    };

    use rend3::AlbedoComponent;
    let albedo = match (data.vertex, data.value, texture) {
        (Some(srgb), Some(value), Some(texture)) => AlbedoComponent::TextureVertexValue {
            texture,
            srgb,
            value,
        },
        (Some(srgb), Some(value), None) => AlbedoComponent::ValueVertex { srgb, value },
        (Some(srgb), None, Some(texture)) => AlbedoComponent::TextureVertex { srgb, texture },
        (Some(srgb), None, None) => AlbedoComponent::Vertex { srgb },
        (None, Some(value), Some(texture)) => AlbedoComponent::TextureValue { texture, value },
        (None, Some(value), None) => AlbedoComponent::Value(value),
        (None, None, Some(texture)) => AlbedoComponent::Texture(texture),
        (None, None, None) => AlbedoComponent::None,
    };

    Ok(albedo)
}

pub fn conv_transparency(data: &schema::Transparency) -> rend3::Transparency {
    match *data {
        schema::Transparency::Opaque => rend3::Transparency::Opaque,
        schema::Transparency::Cutout { cutout } => rend3::Transparency::Cutout { cutout },
        schema::Transparency::Blend => rend3::Transparency::Blend,
    }
}

pub async fn conv_normal(
    store: &AssetStore,
    data: &Option<schema::NormalTexture>,
) -> Result<rend3::NormalTexture> {
    let Some(data) = data.as_ref() else {
        return Ok(rend3::NormalTexture::None);
    };

    let texture = load_texture(store, data.texture).await?;

    use schema::NormalTextureYDirection::*;
    let y_dir = match data.direction {
        Up => rend3::NormalTextureYDirection::Up,
        Down => rend3::NormalTextureYDirection::Down,
    };

    use schema::NormalTextureComponents::*;
    Ok(match data.components {
        Tricomponent => rend3::NormalTexture::Tricomponent(texture, y_dir),
        Bicomponent => rend3::NormalTexture::Bicomponent(texture, y_dir),
        BicomponentSwizzled => rend3::NormalTexture::BicomponentSwizzled(texture, y_dir),
    })
}

pub async fn conv_aomr_textures(
    store: &AssetStore,
    data: &schema::AoMRTextures,
) -> Result<rend3::AoMRTextures> {
    Ok(match data {
        schema::AoMRTextures::None => rend3::AoMRTextures::None,
        schema::AoMRTextures::Combined { texture } => rend3::AoMRTextures::Combined {
            texture: load_optional_texture(store, *texture).await?,
        },
        schema::AoMRTextures::SwizzledSplit {
            ao_texture,
            mr_texture,
        } => rend3::AoMRTextures::SwizzledSplit {
            ao_texture: load_optional_texture(store, *ao_texture).await?,
            mr_texture: load_optional_texture(store, *mr_texture).await?,
        },
        schema::AoMRTextures::Split {
            ao_texture,
            mr_texture,
        } => rend3::AoMRTextures::Split {
            ao_texture: load_optional_texture(store, *ao_texture).await?,
            mr_texture: load_optional_texture(store, *mr_texture).await?,
        },
        schema::AoMRTextures::BWSplit {
            ao_texture,
            m_texture,
            r_texture,
        } => rend3::AoMRTextures::BWSplit {
            ao_texture: load_optional_texture(store, *ao_texture).await?,
            m_texture: load_optional_texture(store, *m_texture).await?,
            r_texture: load_optional_texture(store, *r_texture).await?,
        },
    })
}

pub async fn conv_clearcoat_textures(
    store: &AssetStore,
    data: &schema::ClearcoatTextures,
) -> Result<rend3::ClearcoatTextures> {
    Ok(match data {
        schema::ClearcoatTextures::None => rend3::ClearcoatTextures::None,
        schema::ClearcoatTextures::GltfCombined { texture } => {
            rend3::ClearcoatTextures::GltfCombined {
                texture: load_optional_texture(store, *texture).await?,
            }
        }
        schema::ClearcoatTextures::GltfSplit {
            clearcoat_texture,
            clearcoat_roughness_texture,
        } => rend3::ClearcoatTextures::GltfSplit {
            clearcoat_texture: load_optional_texture(store, *clearcoat_texture).await?,
            clearcoat_roughness_texture: load_optional_texture(store, *clearcoat_roughness_texture)
                .await?,
        },
        schema::ClearcoatTextures::BWSplit {
            clearcoat_texture,
            clearcoat_roughness_texture,
        } => rend3::ClearcoatTextures::BWSplit {
            clearcoat_texture: load_optional_texture(store, *clearcoat_texture).await?,
            clearcoat_roughness_texture: load_optional_texture(store, *clearcoat_roughness_texture)
                .await?,
        },
    })
}

pub async fn load_optional_texture(
    store: &AssetStore,
    id: Option<LumpId>,
) -> Result<Option<ResourceHandle<Texture>>> {
    if let Some(id) = id {
        let texture = load_texture(store, id).await?;
        Ok(Some(texture))
    } else {
        Ok(None)
    }
}

pub async fn load_texture(store: &AssetStore, id: LumpId) -> Result<ResourceHandle<Texture>> {
    Ok(Arc::unwrap_or_clone(
        store.load_asset::<TextureLoader>(&id).await?,
    ))
}

pub async fn conv_material_component<T: Clone>(
    store: &AssetStore,
    data: &schema::MaterialComponent<T>,
) -> Result<rend3::MaterialComponent<T>> {
    let texture = load_optional_texture(store, data.texture).await?;

    Ok(match (data.value.clone(), texture) {
        (Some(value), Some(texture)) => rend3::MaterialComponent::TextureValue { texture, value },
        (Some(value), None) => rend3::MaterialComponent::Value(value),
        (None, Some(texture)) => rend3::MaterialComponent::Texture(texture),
        (None, None) => rend3::MaterialComponent::None,
    })
}

pub fn conv_sample_type(data: &schema::SampleType) -> rend3::SampleType {
    match data {
        schema::SampleType::Nearest => rend3::SampleType::Nearest,
        schema::SampleType::Linear => rend3::SampleType::Linear,
    }
}
