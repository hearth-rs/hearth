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

use std::f32::consts::FRAC_PI_2;

use glam::{EulerRot, Mat4, Quat, Vec3};
use hearth_guest::{window::*, *};

#[no_mangle]
pub extern "C" fn run() {
    let window = REGISTRY.get_service(SERVICE_NAME).unwrap();
    let events = Mailbox::new();
    let events_cap = events.make_capability(Permissions::SEND);
    let mut flycam = Flycam::new(window.clone());

    window.send_json(&WindowCommand::Subscribe, &[&events_cap]);
    window.send_json(&WindowCommand::SetCursorVisible(false), &[]);
    window.send_json(&WindowCommand::SetCursorGrab(CursorGrabMode::Locked), &[]);

    loop {
        let (event, _) = events.recv_json::<WindowEvent>();
        flycam.on_event(event);
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
