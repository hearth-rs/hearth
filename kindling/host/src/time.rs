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

use super::*;

lazy_static::lazy_static! {
    static ref SLEEP_SERVICE: Capability = {
        registry::REGISTRY.get_service("hearth.Sleep").unwrap()
    };

    static ref TIMER_FACTORY: RequestResponse<(), ()> = {
        RequestResponse::new(registry::REGISTRY.get_service("hearth.TimerFactory").unwrap())
    };

    static ref STOPWATCH_FACTORY: RequestResponse<(), ()> = {
        RequestResponse::new(registry::REGISTRY.get_service("hearth.StopwatchFactory").unwrap())
    };
}

/// Sleeps for the given time in seconds.
pub fn sleep(duration: f32) {
    let reply = Mailbox::new();
    let reply_cap = reply.make_capability(Permissions::SEND);
    reply.monitor(&SLEEP_SERVICE);

    SLEEP_SERVICE.send_json(&duration, &[&reply_cap]);

    let _ = reply.recv();
}

pub struct Timer(RequestResponse<f32, ()>);

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

impl Timer {
    /// Creates a new Timer.
    pub fn new() -> Self {
        let (_, resp) = TIMER_FACTORY.request((), &[]);
        Self(RequestResponse::new(resp.get(0).unwrap().clone()))
    }

    /// Sleeps the given time in seconds from the end of the last tick.
    pub fn tick(&self, duration: f32) {
        self.0.request(duration, &[]);
    }
}

pub struct Stopwatch(RequestResponse<(), f32>);

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

impl Stopwatch {
    /// Creates a new Stopwatch.
    pub fn new() -> Self {
        let (_, resp) = STOPWATCH_FACTORY.request((), &[]);
        Self(RequestResponse::new(resp.get(0).unwrap().clone()))
    }

    /// Responds with the time since the last request.
    pub fn lap(&self) -> f32 {
        self.0.request((), &[]).0
    }
}
