use std::f32::consts::PI;

use glam::{uvec2, vec3, Mat4, Vec2, Vec3, Vec4};
use hearth_guest::{renderer::*, ByteVec, Lump, LumpId};
use image::GenericImageView;
use kindling_host::prelude::{RequestResponse, REGISTRY};
use serde::Serialize;

pub type Renderer = RequestResponse<RendererRequest, RendererResponse>;

#[no_mangle]
pub extern "C" fn run() {
    let ren = REGISTRY.get_service("hearth.Renderer").unwrap();
    let ren = Renderer::new(ren);

    let _ = ren.request(
        RendererRequest::SetAmbientLighting {
            ambient: Vec4::new(1.0, 1.0, 1.0, 1.0),
        },
        &[],
    );

    spawn_gltf(
        &ren,
        include_bytes!("korakoe.vrm"),
        Mat4::from_translation(vec3(-2.0, -1.0, 1.7)) * Mat4::from_rotation_y(PI / -2.0),
    );
}

pub fn load_albedo_material(texture: &[u8]) -> LumpId {
    json_lump(&MaterialData {
        albedo: load_texture(texture),
    })
}

pub fn spawn_gltf(ren: &Renderer, src: &[u8], transform: Mat4) {
    use gltf::*;

    let (document, buffers, images) = import_slice(src).unwrap();

    let images: Vec<_> = images
        .into_iter()
        .map(|image| {
            json_lump(&TextureData {
                label: None,
                data: image.pixels.clone(),
                size: uvec2(image.width, image.height),
            })
        })
        .collect();

    let materials: Vec<_> = document
        .materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            let base = pbr.base_color_texture().unwrap();
            let base = base.texture().source();
            let albedo = images[base.index()];
            json_lump(&MaterialData { albedo })
        })
        .collect();

    let mut objects = Vec::new();

    for mesh in document.meshes() {
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

pub fn load_texture(src: &[u8]) -> LumpId {
    let image = image::load_from_memory(src).unwrap();
    let size = image.dimensions().into();
    let data = image.into_rgba8().into_vec();

    json_lump(&TextureData {
        label: None,
        data,
        size,
    })
}

pub fn json_lump(data: &impl Serialize) -> LumpId {
    let data = serde_json::to_vec(data).unwrap();
    Lump::load(&data).get_id()
}
