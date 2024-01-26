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

use hearth_guest::{
    debug_draw::{DebugDrawMesh, DebugDrawVertex},
    export_metadata,
    window::*,
    Color,
};
use kindling_host::prelude::{
    glam::{vec3, DVec2, EulerRot, Mat4, Quat, Vec3},
    *,
};
use rapier3d::{geometry::ColliderSet, parry::query::Ray, pipeline::QueryPipeline, prelude::*};

export_metadata!();

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

struct Cube {
    position: Vec3,
    radius: f32,
    dd: DebugDraw,
}

impl Cube {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            radius: 1.0,
            dd: DebugDraw::new(),
        }
    }

    fn make_collider(&self) -> Collider {
        ColliderBuilder::cuboid(self.radius, self.radius, self.radius)
            .position(self.position.to_array().into())
            .build()
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

        self.dd.update(DebugDrawMesh { vertices, indices })
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
            let handle = physics.colliders.insert(cube.make_collider());
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
    let events = MAIN_WINDOW.subscribe();
    let mut flycam = Flycam::new();
    let mut cubes = CubeWorld::new();

    MAIN_WINDOW.hide_cursor();
    MAIN_WINDOW.cursor_grab_mode(CursorGrabMode::Locked);

    loop {
        let (event, _) = events.recv::<WindowEvent>();
        flycam.on_event(event);

        let (origin, direction) = flycam.cast_ray();
        cubes.update(origin, direction);
    }
}

struct Flycam {
    keys: Keys,
    position: Vec3,
    pitch: f32,
    yaw: f32,
    cursor_pos: DVec2,
    window_size: DVec2,
}

impl Flycam {
    pub fn new() -> Self {
        Self {
            keys: Keys::empty(),
            position: Vec3::ZERO,
            pitch: 0.0,
            yaw: 0.0,
            cursor_pos: DVec2::ZERO,
            window_size: DVec2::ZERO,
        }
    }

    pub fn cast_ray(&self) -> (Vec3, Vec3) {
        let orientation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        let rotation = Mat4::from_quat(orientation);
        let delta = if self.keys.contains(Keys::UNLOCK) {
            let screen_pos = self.cursor_pos / self.window_size;
            let view_space = screen_pos * 2.0 - 1.0;
            let x = view_space.x * (self.window_size.x / self.window_size.y);
            let y = -view_space.y;
            vec3(x as f32, y as f32, -1.0)
        } else {
            -Vec3::Z
        };

        (self.position, rotation.transform_vector3(delta))
    }

    pub fn on_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::Redraw { dt } => {
                if self.keys.contains(Keys::UNLOCK) {
                    return;
                }
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

                MAIN_WINDOW.set_camera(90.0, 0.01, view);
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
                    VirtualKeyCode::Tab => Keys::UNLOCK,
                    _ => return,
                };

                match state {
                    ElementState::Pressed => self.keys |= mask,
                    ElementState::Released => self.keys &= !mask,
                }
                if self.keys.contains(Keys::UNLOCK) {
                    MAIN_WINDOW.show_cursor();
                    MAIN_WINDOW.cursor_grab_mode(CursorGrabMode::None);
                } else {
                    MAIN_WINDOW.hide_cursor();
                    MAIN_WINDOW.cursor_grab_mode(CursorGrabMode::Locked);
                }
            }
            WindowEvent::MouseMotion(delta) => {
                if self.keys.contains(Keys::UNLOCK) {
                    return;
                }
                self.yaw += -delta.x as f32 * 0.003;
                self.pitch += -delta.y as f32 * 0.003;
                self.pitch = self.pitch.clamp(-FRAC_PI_2, FRAC_PI_2);
            }
            WindowEvent::Resized(size) => self.window_size = size.as_dvec2(),
            WindowEvent::CursorMoved { position } => self.cursor_pos = position,
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
        const UNLOCK = 1 << 6;
    }
}
