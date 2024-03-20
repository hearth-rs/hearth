use glam::{uvec2, Mat4, Vec2, Vec3, Vec4};
use gltf::image::Format;
use hearth_guest::{renderer::*, ByteVec, Lump, LumpId, PARENT};
use kindling_host::prelude::*;
use kindling_schema::{gltf::GltfRequest, model::Model};

pub type Renderer = RequestResponse<RendererRequest, RendererResponse>;

#[no_mangle]
pub extern "C" fn run() {
    loop {
        let (req, caps) = PARENT.recv::<GltfRequest>();
        let child = spawn_fn(on_request, None);
        child.send(&req, caps.iter().collect::<Vec<_>>().as_slice());
    }
}

pub fn on_request() {
    let (req, caps) = PARENT.recv::<GltfRequest>();

    let reply = caps.first().expect("no reply cap");

    match req {
        GltfRequest::LoadSingle { lump, transform } => {
            let src = Lump::load_by_id(&lump).get_data();
            let model = load_gltf(&src, transform);
            reply.send(&Result::<Model, String>::Ok(model), &[]);
        }
    }
}

pub fn load_material(images: &[LumpId], material: &gltf::Material) -> MaterialData {
    let pbr = material.pbr_metallic_roughness();
    let base = pbr.base_color_texture().unwrap();
    let base = base.texture().source();
    let albedo = images[base.index()];

    let ao = material
        .occlusion_texture()
        .map(|info| images[info.texture().source().index()]);

    let mr = pbr
        .metallic_roughness_texture()
        .map(|info| images[info.texture().source().index()]);

    let normal = if let Some(info) = material.normal_texture() {
        let texture = images[info.texture().source().index()];
        let direction = NormalTextureYDirection::Up;
        let components = NormalTextureComponents::Tricomponent;

        Some(NormalTexture {
            texture,
            direction,
            components,
        })
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

    let emissive_texture = material
        .emissive_texture()
        .map(|info| images[info.texture().source().index()]);

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

pub fn load_gltf(src: &[u8], transform: Mat4) -> Model {
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

            Lump::load(&TextureData {
                label: None,
                data,
                size: uvec2(image.width, image.height),
            })
            .get_id()
        })
        .collect();

    let materials: Vec<_> = document
        .materials()
        .map(|material| Lump::load(&load_material(&images, &material)))
        .collect();

    let mut meshes = Vec::new();
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

            let mesh = Lump::load(&MeshData {
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

            let material = &materials[prim.material().index().unwrap()];

            meshes.push(kindling_schema::model::Mesh {
                mesh: mesh.get_id(),
                material: material.get_id(),
                transform,
            });
        }
    }

    Model { meshes }
}
