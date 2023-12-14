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

use std::marker::PhantomData;

use hearth_guest::{Capability, Mailbox, Permissions};
use serde::{Deserialize, Serialize};

pub use glam;

pub mod canvas;
pub mod debug_draw;
pub mod fs;
pub mod registry;
pub mod terminal;
pub mod time;
pub mod wasm;
pub mod window;

/// A convenience module to import all of the most important host-side structures.
///
/// Use with:
///
/// ```rs
/// use kindling_host::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        canvas::Canvas,
        debug_draw::DebugDraw,
        fs::{get_file, list_files, read_file},
        glam,
        registry::REGISTRY,
        terminal::Terminal,
        time::{sleep, Stopwatch, Timer},
        wasm::{spawn_fn, spawn_mod},
        window::MAIN_WINDOW,
        RequestResponse, {debug, error, info, log, trace, warning},
    };
}

/// A helper struct for request-response capabilities.
pub struct RequestResponse<Request, Response> {
    cap: Capability,
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
}

impl<Request, Response> AsRef<Capability> for RequestResponse<Request, Response> {
    fn as_ref(&self) -> &Capability {
        &self.cap
    }
}

impl<Request, Response> RequestResponse<Request, Response>
where
    Request: Serialize,
    Response: for<'a> Deserialize<'a>,
{
    /// Wrap a raw capability with the request-response API.
    pub const fn new(cap: Capability) -> Self {
        Self {
            cap,
            _request: PhantomData,
            _response: PhantomData,
        }
    }

    /// Perform a request on this capability.
    ///
    /// Fails if the capability is unavailable.
    pub fn request(&self, request: Request, args: &[&Capability]) -> (Response, Vec<Capability>) {
        let reply = Mailbox::new();
        let reply_cap = reply.make_capability(Permissions::SEND);
        reply.monitor(&self.cap);

        let mut caps = Vec::with_capacity(args.len() + 1);
        caps.push(&reply_cap);
        caps.extend_from_slice(args);

        self.cap.send_json(&request, caps.as_slice());

        reply.recv_json()
    }
}

#[macro_export]
macro_rules! log {
    ($level:expr, $($arg:tt)*) => {
        ::hearth_guest::log(
            $level,
            ::core::module_path!(),
            &format!($($arg)*),
        )
    }
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        log!(::hearth_guest::ProcessLogLevel::Trace, $($arg)*);
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        log!(::hearth_guest::ProcessLogLevel::Debug, $($arg)*);
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        log!(::hearth_guest::ProcessLogLevel::Info, $($arg)*);
    };
}

#[macro_export]
macro_rules! warning {
    ($($arg:tt)*) => {
        log!(::hearth_guest::ProcessLogLevel::Warning, $($arg)*);
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        log!(::hearth_guest::ProcessLogLevel::Error, $($arg)*);
    };
}
