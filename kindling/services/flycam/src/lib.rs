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

use std::{collections::HashMap, f32::consts::FRAC_PI_2};

use glam::{vec3, EulerRot, Mat4, Quat, Vec3};
use hearth_guest::{
    debug_draw::{DebugDrawMesh, DebugDrawUpdate, DebugDrawVertex},
    window::*,
    *,
};
use rapier3d::{geometry::ColliderSet, parry::query::Ray, pipeline::QueryPipeline, prelude::*};

struct PhysicsWorld {
    bodies: RigidBodySet,
    colliders: ColliderSet,
    qp: QueryPipeline,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            qp: QueryPipeline::new(),
        }
    }
}

#[derive(Clone)]
struct Cube {
    position: Vec3,
    radius: f32,
    dd: Capability,
}

impl Into<Collider> for Cube {
    fn into(self) -> Collider {
        ColliderBuilder::cuboid(self.radius, self.radius, self.radius)
            .position(self.position.to_array().into())
            .build()
    }
}

impl Cube {
    pub fn new(position: Vec3) -> Self {
        let dd_factory = RequestResponse::<(), ()>::new(
            REGISTRY.get_service("hearth.DebugDrawFactory").unwrap(),
        );

        let (_, caps) = dd_factory.request((), &[]);
        let dd = caps.get(0).unwrap().clone();

        Self {
            position,
            radius: 1.0,
            dd,
        }
    }

    pub fn draw(&self, color: Color) {
        let r = self.radius;
        let vertices = [
            vec3(r, r, r),
            vec3(r, r, -r),
            vec3(r, -r, r),
            vec3(r, -r, -r),
            vec3(-r, r, r),
            vec3(-r, r, -r),
            vec3(-r, -r, r),
            vec3(-r, -r, -r),
        ]
        .iter()
        .map(|v| *v + self.position)
        .map(|v| DebugDrawVertex { position: v, color })
        .collect();

        let indices = (0..8)
            .flat_map(|limit| (limit..8).flat_map(move |idx| [limit, idx]))
            .collect();

        self.dd.send_json(
            &DebugDrawUpdate::Contents(DebugDrawMesh { vertices, indices }),
            &[],
        );
    }
}

struct CubeWorld {
    physics: PhysicsWorld,
    cubes: HashMap<ColliderHandle, Cube>,
    current_highlight: Option<ColliderHandle>,
    selected_color: Color,
    unselected_color: Color,
}

impl CubeWorld {
    pub fn new() -> Self {
        let mut physics = PhysicsWorld::new();
        let mut cubes = HashMap::new();

        let positions = [
            vec3(4.0, 5.0, -2.0),
            vec3(-2.0, 5.0, 4.0),
            vec3(-3.0, 3.0, -1.0),
        ];

        for position in positions {
            let cube = Cube::new(position);
            let handle = physics.colliders.insert(cube.clone());
            cubes.insert(handle, cube);
        }

        physics.qp.update(&physics.bodies, &physics.colliders);

        let selected_color = Color::from_rgb(0xff, 0x00, 0x00);
        let unselected_color = Color::from_rgb(0x00, 0x00, 0xff);

        for cube in cubes.values() {
            cube.draw(unselected_color);
        }

        Self {
            cubes,
            physics,
            current_highlight: None,
            selected_color,
            unselected_color,
        }
    }

    pub fn update(&mut self, ray_origin: Vec3, ray_delta: Vec3) {
        let ray = Ray::new(ray_origin.to_array().into(), ray_delta.to_array().into());

        let new_highlight = self
            .physics
            .qp
            .cast_ray(
                &self.physics.bodies,
                &self.physics.colliders,
                &ray,
                10.0,
                true,
                QueryFilter::new(),
            )
            .map(|cast| cast.0);

        if new_highlight != self.current_highlight {
            if let Some(old) = self.current_highlight {
                self.cubes.get(&old).unwrap().draw(self.unselected_color);
            }

            if let Some(new) = new_highlight {
                self.cubes.get(&new).unwrap().draw(self.selected_color);
            }

            self.current_highlight = new_highlight;
        }
    }
}

#[no_mangle]
pub extern "C" fn run() {
    let window = REGISTRY.get_service(SERVICE_NAME).unwrap();
    let events = Mailbox::new();
    let events_cap = events.make_capability(Permissions::SEND);
    let mut flycam = Flycam::new(window.clone());
    let mut cubes = CubeWorld::new();

    window.send_json(&WindowCommand::Subscribe, &[&events_cap]);
    window.send_json(&WindowCommand::SetCursorVisible(false), &[]);
    window.send_json(&WindowCommand::SetCursorGrab(CursorGrabMode::Locked), &[]);

    loop {
        let (event, _) = events.recv_json::<WindowEvent>();
        flycam.on_event(event);

        let (origin, direction) = flycam.cast_ray();
        cubes.update(origin, direction);
    }
}

struct Flycam {
    window: Capability,
    keys: Keys,
    position: Vec3,
    pitch: f32,
    yaw: f32,
}

impl Flycam {
    pub fn new(window: Capability) -> Self {
        Self {
            window,
            keys: Keys::empty(),
            position: Vec3::ZERO,
            pitch: 0.0,
            yaw: 0.0,
        }
    }

    pub fn cast_ray(&self) -> (Vec3, Vec3) {
        let orientation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        let rotation = Mat4::from_quat(orientation);
        (self.position, rotation.transform_vector3(-Vec3::Z))
    }

    pub fn on_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::Redraw { dt } => {
                let orientation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
                // view matrix is inverted camera pose (world space to camera space)
                let rotation = Mat4::from_quat(orientation.inverse());
                let translation = Mat4::from_translation(-self.position);
                let view = rotation * translation;

                // move the camera
                let mut movement = Vec3::ZERO;

                if self.keys.contains(Keys::LEFT) {
                    movement.x -= 1.0;
                }

                if self.keys.contains(Keys::RIGHT) {
                    movement.x += 1.0;
                }

                if self.keys.contains(Keys::FORWARD) {
                    movement.z -= 1.0;
                }

                if self.keys.contains(Keys::BACK) {
                    movement.z += 1.0;
                }

                let speed = 4.0;

                self.position += orientation * movement * dt * speed;

                if self.keys.contains(Keys::DOWN) {
                    self.position.y -= dt * speed;
                }

                if self.keys.contains(Keys::UP) {
                    self.position.y += dt * speed;
                }

                self.window.send_json(
                    &WindowCommand::SetCamera {
                        vfov: 90.0,
                        near: 0.01,
                        view,
                    },
                    &[],
                );
            }
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        state,
                        virtual_keycode: Some(keycode),
                        ..
                    },
                ..
            } => {
                let mask = match keycode {
                    VirtualKeyCode::W => Keys::FORWARD,
                    VirtualKeyCode::A => Keys::LEFT,
                    VirtualKeyCode::S => Keys::BACK,
                    VirtualKeyCode::D => Keys::RIGHT,
                    VirtualKeyCode::E => Keys::UP,
                    VirtualKeyCode::Q => Keys::DOWN,
                    _ => return,
                };

                match state {
                    ElementState::Pressed => self.keys |= mask,
                    ElementState::Released => self.keys &= !mask,
                }
            }
            WindowEvent::MouseMotion(delta) => {
                self.yaw += -delta.x as f32 * 0.003;
                self.pitch += -delta.y as f32 * 0.003;
                self.pitch = self.pitch.clamp(-FRAC_PI_2, FRAC_PI_2);
            }
            _ => {}
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug)]
     pub struct Keys: u32 {
        const LEFT = 1 << 0;
        const RIGHT = 1 << 1;
        const FORWARD = 1 << 2;
        const BACK = 1 << 3;
        const DOWN = 1 << 4;
        const UP = 1 << 5;
    }
}
