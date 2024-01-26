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

use std::collections::{HashMap, HashSet};

use hearth_guest::{export_metadata, Capability, Mailbox, Permissions, PARENT};
use kindling_host::prelude::*;
use kindling_schema::physics::*;
use rapier3d::prelude::*;

export_metadata!();

pub struct RigidBodyInstance {
    pub sink: Mailbox,
    pub ports: HashSet<String>,
    pub port_handler: Capability,
}

#[derive(Default)]
pub struct World {
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: BroadPhase,
    pub narrow_phase: NarrowPhase,
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub query_pipeline: QueryPipeline,
    pub body_instances: HashMap<RigidBodyHandle, RigidBodyInstance>,
}

impl World {
    pub fn run(mut self) {
        let dt = self.integration_parameters.dt;
        let timer = Timer::new();

        loop {
            self.flush_requests();
            self.step();
            timer.tick(dt);
        }
    }

    pub fn step(&mut self) {
        let gravity = Default::default();

        self.physics_pipeline.step(
            &gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            Some(&mut self.query_pipeline),
            &(),
            &(),
        );
    }

    pub fn update_ports(&mut self) {
        let target = "transform".to_string();

        for handle in self.island_manager.active_dynamic_bodies().iter() {
            let instance = self.body_instances.get(handle).unwrap();

            if !instance.ports.contains(&target) {
                continue;
            }

            let body = self.rigid_body_set.get(*handle).unwrap();

            let _transform = Transform {
                translation: (*body.translation()).into(),
                rotation: (*body.rotation()).into(),
            };

            instance.port_handler.send(
                &PortEvent {
                    collider: None,
                    target: target.clone(),
                    data: (),
                },
                &[],
            );
        }
    }

    pub fn flush_requests(&mut self) {
        while let Some((request, caps)) = PARENT.try_recv_raw() {
            let data = match serde_json::from_slice(&request) {
                Ok(data) => data,
                Err(_) => continue,
            };

            let Some(reply) = caps.first() else {
                continue;
            };

            self.on_request(data, reply, &caps[1..]);
        }
    }

    pub fn on_request(&mut self, request: Request, reply: &Capability, args: &[Capability]) {
        use Request::*;
        match request {
            AddRigidBody(request) => {
                let port_handler = &args[0];
                let body = self.add_rigid_body(request, port_handler);
                reply.send(&(), &[&body]);
            }
            Query(request) => {
                let response = self.query(request);
                reply.send(&response, &[]);
            }
        }
    }

    pub fn add_rigid_body(
        &mut self,
        request: AddRigidBody,
        port_handler: &Capability,
    ) -> Capability {
        use RigidBodyKind::*;
        let body_type = match request.kind {
            Dynamic => RigidBodyType::Dynamic,
            Fixed => RigidBodyType::Fixed,
            KinematicPositionBased => RigidBodyType::KinematicPositionBased,
            KinematicVelocityBased => RigidBodyType::KinematicVelocityBased,
        };

        let position = Isometry {
            translation: request.transform.translation.into(),
            rotation: request.transform.rotation.into(),
        };

        let body_builder = RigidBodyBuilder::new(body_type).position(position);

        let body = self.rigid_body_set.insert(body_builder);

        for collider in request.colliders {
            let shape = self.conv_shape(collider.shape);

            let builder = ColliderBuilder::new(shape);

            let _handle =
                self.collider_set
                    .insert_with_parent(builder, body, &mut self.rigid_body_set);
        }

        let instance = RigidBodyInstance {
            sink: Mailbox::new(),
            ports: HashSet::from_iter(request.ports),
            port_handler: port_handler.to_owned(),
        };

        let sink = instance.sink.make_capability(Permissions::SEND);

        self.body_instances.insert(body, instance);

        sink
    }

    pub fn conv_shape(&self, shape: ShapeKind) -> SharedShape {
        match shape {
            ShapeKind::Cuboid { half_extents } => {
                SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z)
            }
        }
    }

    pub fn query(&mut self, query: Query) -> QueryResult {
        match query {
            Query::Ray {
                origin, direction, ..
            } => {
                let ray = Ray::new(origin.into(), direction.into());
                let filter = QueryFilter::new();

                let result = self.query_pipeline.cast_ray_and_get_normal(
                    &self.rigid_body_set,
                    &self.collider_set,
                    &ray,
                    1.0,
                    true,
                    filter,
                );

                let items = if result.is_some() {
                    vec![QueryItem {}]
                } else {
                    vec![]
                };

                QueryResult { items }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn run() {
    let world = World::default();

    world.run();
}
