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

use std::marker::PhantomData;

use glam::{Quat, Vec3};
use hearth_guest::{Capability, Mailbox, Permissions};
use serde::{Deserialize, Serialize};

// we're workshopping some cleaner protocol definition code in this file,
// hence the huge macros and stuff.

///////////////////////////////////////////////////////////////////////////////
/////////////////////////////////// core code /////////////////////////////////
///////////////////////////////////////////////////////////////////////////////

pub trait RequestVariant<T>: From<T> + Serialize {
    type Response: for<'a> Deserialize<'a>;
}

#[macro_export]
macro_rules! def_protocol {
    { $(
        $(#[$($attrss:tt)*])*
        $request:ident -> $response:ty
    ),* } => {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        pub enum Request {
            $($request($request)),*
        }

        $(
            impl RequestVariant<$request> for Request {
                type Response = $response;
            }

            impl From<$request> for Request {
                fn from(req: $request) -> Request {
                    Request::$request(req)
                }
            }
        )*
    };
}

///////////////////////////////////////////////////////////////////////////////
//////////////////////////////// utility code /////////////////////////////////
///////////////////////////////////////////////////////////////////////////////

pub struct RequestResponse<T>(Capability, PhantomData<T>);

impl<T> RequestResponse<T> {
    pub fn request<R>(&self, request: R, args: &[&Capability]) -> (T::Response, Vec<Capability>)
    where
        T: RequestVariant<R>,
    {
        let reply = Mailbox::new();
        let reply_cap = reply.make_capability(Permissions::SEND);
        reply.monitor(&self.0);

        let data = T::from(request);

        let mut caps = vec![&reply_cap];
        caps.extend_from_slice(args);

        self.0.send(&data, caps.as_slice());

        reply.recv()
    }
}

///////////////////////////////////////////////////////////////////////////////
/////////////////////////////////// user code /////////////////////////////////
///////////////////////////////////////////////////////////////////////////////

def_protocol! {
    AddRigidBody -> (),
    Query -> QueryResult
}

/// An event from a port.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PortEvent {
    /// The target collider of this event, or `None` if the body is the target.
    pub collider: Option<usize>,

    /// The name of the port this event targets.
    pub target: String,

    /// The inner event data.
    pub data: (),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddRigidBody {
    pub kind: RigidBodyKind,

    /// The initial transform of this rigid body.
    pub transform: Transform,

    /// The port names that this rigid body subscribes to.
    pub ports: Vec<String>,

    /// The list of colliders on this rigid body.
    pub colliders: Vec<Collider>,
}

/// A transform matrix.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
}

/// The status of a body, governing the way it is affected by external forces.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RigidBodyKind {
    /// Can be affected by external forces.
    Dynamic,

    /// Cannot be affected by external forces.
    Fixed,

    /// Not affected by external forces but can be controlled at the position
    /// level with realistic one-way interaction with dynamic bodies.
    KinematicPositionBased,

    /// Not affected by external forces but can be controlled at the velocity
    /// level with realistic one-way interaction with dynamic bodies.
    KinematicVelocityBased,
}

/// Initialization data for a new collider.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Collider {
    /// This collider's shape.
    pub shape: ShapeKind,

    /// The port names that this collider subscribes to.
    pub ports: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ShapeKind {
    /// Shape of a box.
    Cuboid {
        /// The half-extents of the cuboid.
        half_extents: Vec3,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Query {
    /// A ray query.
    Ray {
        /// The ray's origin.
        origin: Vec3,

        /// The ray's direction.
        ///
        /// Note that this includes the magnitude of the ray.
        direction: Vec3,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryResult {
    pub items: Vec<QueryItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryItem {}
