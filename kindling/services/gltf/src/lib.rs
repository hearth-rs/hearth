use std::{f32::consts::PI, path::Path};

use glam::{uvec2, vec3, Mat3, Mat4, Vec2, Vec3, Vec4};
use gltf::image::Format;
use hearth_guest::{renderer::*, ByteVec, Lump, LumpId};
use kindling_host::{
    fs,
    prelude::{RequestResponse, REGISTRY},
};
use serde::Serialize;

pub type Renderer = RequestResponse<RendererRequest, RendererResponse>;

#[no_mangle]
pub extern "C" fn run() {
    let ren = REGISTRY.get_service("hearth.Renderer").unwrap();
    let ren = Renderer::new(ren);

    let _ = ren.request(
        RendererRequest::SetAmbientLighting {
            ambient: Vec4::new(0.1, 0.1, 0.1, 1.0),
        },
        &[],
    );

    let _ = ren.request(
        RendererRequest::AddDirectionalLight {
            initial_state: DirectionalLightState {
                color: Vec3::ONE,
                intensity: 10.0,
                direction: Vec3::new(0.1, -1.0, 0.1).normalize(),
                distance: 10.0,
            },
        },
        &[],
    );

    spawn_gltf(
        &ren,
        include_bytes!("WaterBottle.glb"),
        Mat4::from_translation(vec3(0.0, -1.0, 0.0)),
    );

    spawn_gltf(
        &ren,
        include_bytes!("DamagedHelmet.glb"),
        Mat4::from_translation(vec3(2.0, -1.0, 1.7)) * Mat4::from_rotation_y(PI / 2.0),
    );

    spawn_gltf(
        &ren,
        include_bytes!("korakoe.vrm"),
        Mat4::from_translation(vec3(-2.0, -1.0, 1.7)) * Mat4::from_rotation_y(PI / -2.0),
    );
}

pub fn spawn_from_fs(ren: &Renderer, path: &str, translation: Mat4) {
    let base_data = fs::read_file(&path).unwrap();
    let base = gltf::Gltf::from_slice_without_validation(&base_data).unwrap();
}

pub fn load_material(images: &[LumpId], material: &gltf::Material) -> MaterialData {
    let pbr = material.pbr_metallic_roughness();
    let base = pbr.base_color_texture().unwrap();
    let base = base.texture().source();
    let albedo = images[base.index()];

    let ao = if let Some(info) = material.occlusion_texture() {
        Some(images[info.texture().source().index()])
    } else {
        None
    };

    let mr = if let Some(info) = pbr.metallic_roughness_texture() {
        Some(images[info.texture().source().index()])
    } else {
        None
    };

    let normal = if let Some(info) = material.normal_texture() {
        let texture = images[info.texture().source().index()];
        let direction = NormalTextureYDirection::Up;
        let components = NormalTextureComponents::Tricomponent;

        Some(NormalTexture {
            texture,
            direction,
            components,
        });

        None
    } else {
        None
    };

    let aomr_textures = match (ao, mr) {
        (Some(ao), Some(mr)) if mr == ao => AoMRTextures::Combined { texture: Some(mr) },
        (ao, mr) => AoMRTextures::SwizzledSplit {
            ao_texture: ao,
            mr_texture: mr,
        },
    };

    let transparency = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => Transparency::Opaque,
        gltf::material::AlphaMode::Mask => Transparency::Cutout {
            cutout: material.alpha_cutoff().unwrap_or(0.5),
        },
        gltf::material::AlphaMode::Blend => Transparency::Blend,
    };

    let emissive_texture = if let Some(info) = material.emissive_texture() {
        Some(images[info.texture().source().index()])
    } else {
        None
    };

    MaterialData {
        albedo: AlbedoComponent {
            vertex: None,
            value: None,
            texture: Some(albedo),
        },
        transparency,
        normal,
        aomr_textures,
        ao_factor: None,
        metallic_factor: Some(pbr.metallic_factor()),
        roughness_factor: Some(pbr.roughness_factor()),
        clearcoat_textures: ClearcoatTextures::None,
        clearcoat_factor: None,
        clearcoat_roughness_factor: None,
        emissive: MaterialComponent {
            value: Some(Vec3::from_slice(&material.emissive_factor())),
            texture: emissive_texture,
        },
        reflectance: MaterialComponent {
            value: None,
            texture: None,
        },
        anisotropy: MaterialComponent {
            value: None,
            texture: None,
        },
        uv_transform0: Default::default(),
        uv_transform1: Default::default(),
        unlit: material.unlit(),
        sample_type: SampleType::Linear,
    }
}

pub fn spawn_gltf(ren: &Renderer, src: &[u8], transform: Mat4) {
    use gltf::*;

    let (document, buffers, images) = import_slice(src).unwrap();

    let images: Vec<_> = images
        .into_iter()
        .map(|image| {
            let mut data = Vec::with_capacity((image.width * image.height) as usize * 4);

            match image.format {
                Format::R8G8B8A8 => data = image.pixels.clone(),
                Format::R8G8B8 => {
                    for rgb in image.pixels.chunks_exact(3) {
                        data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
                    }
                }
                _ => panic!("unsupported format"),
            }

            json_lump(&TextureData {
                label: None,
                data,
                size: uvec2(image.width, image.height),
            })
        })
        .collect();

    let materials: Vec<_> = document
        .materials()
        .map(|material| json_lump(&load_material(&images, &material)))
        .collect();

    let mut objects = Vec::new();
    let scene = document.default_scene().expect("no default scene");
    let mut node_stack = Vec::new();
    node_stack.extend(scene.nodes().map(|node| (node, transform)));

    while let Some((node, transform)) = node_stack.pop() {
        let transform = transform * Mat4::from_cols_array_2d(&node.transform().matrix());

        node_stack.extend(node.children().map(|node| (node, transform)));

        let Some(mesh) = node.mesh() else {
            continue;
        };

        for prim in mesh.primitives() {
            let reader = prim.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<_> = reader
                .read_positions()
                .expect("glTF primitive has no positions")
                .map(Vec3::from)
                .collect();

            let len = positions.len();

            let mut normals = vec![Default::default(); len];
            let mut tangents = vec![Default::default(); len];
            let mut uv0 = vec![Default::default(); len];
            let mut uv1 = vec![Default::default(); len];
            let mut colors = vec![Default::default(); len];
            let mut joint_indices = vec![Default::default(); len];
            let mut joint_weights = vec![Default::default(); len];
            let mut indices = Vec::new();

            if let Some(read_normals) = reader.read_normals() {
                normals.clear();
                normals.extend(read_normals.map(Vec3::from));
            }

            if let Some(read_tangents) = reader.read_tangents() {
                tangents.clear();

                tangents.extend(read_tangents.map(|t| Vec3::from_slice(&t)));
            }

            if let Some(read_uv0) = reader.read_tex_coords(0) {
                uv0.clear();
                uv0.extend(read_uv0.into_f32().map(Vec2::from));
            }

            if let Some(read_uv1) = reader.read_tex_coords(1) {
                uv1.clear();
                uv1.extend(read_uv1.into_f32().map(Vec2::from));
            }

            if let Some(read_colors) = reader.read_colors(0) {
                colors.clear();
                colors.extend(read_colors.into_rgba_u8());
            }

            if let Some(joints) = reader.read_joints(0) {
                joint_indices.clear();
                joint_indices.extend(joints.into_u16());
            }

            if let Some(read_weights) = reader.read_weights(0) {
                joint_weights.clear();
                joint_weights.extend(read_weights.into_f32().map(Vec4::from));
            }

            if let Some(read_indices) = reader.read_indices() {
                indices.extend(read_indices.into_u32());
            }

            let mesh = json_lump(&MeshData {
                positions: ByteVec(positions),
                normals: ByteVec(normals),
                tangents: ByteVec(tangents),
                uv0: ByteVec(uv0),
                uv1: ByteVec(uv1),
                colors: ByteVec(colors),
                joint_indices: ByteVec(joint_indices),
                joint_weights: ByteVec(joint_weights),
                indices: ByteVec(indices),
            });

            let material = materials[prim.material().index().unwrap()];

            objects.push(RendererRequest::AddObject {
                mesh,
                skeleton: None,
                material,
                transform,
            });
        }
    }

    for object in objects {
        ren.request(object, &[]).0.unwrap();
    }
}

pub fn json_lump(data: &impl Serialize) -> LumpId {
    let data = serde_json::to_vec(data).unwrap();
    Lump::load(&data).get_id()
}
