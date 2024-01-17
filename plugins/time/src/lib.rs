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

use hearth_runtime::{
    async_trait, cargo_process_metadata,
    flue::Table,
    process::ProcessMetadata,
    runtime::{Plugin, RuntimeBuilder},
    tokio::{
        self,
        time::{Duration, Instant},
    },
    tracing::debug,
    utils::{
        MessageInfo, RequestInfo, RequestResponseProcess, ResponseInfo, RunnerContext,
        ServiceRunner, SinkProcess,
    },
};

/// A plugin that provides timing services to guests.
///
/// Adds the [SleepService], [TimerFactory], and [StopwatchFactory] services.
#[derive(Default)]
pub struct TimePlugin;

impl Plugin for TimePlugin {
    fn build(&mut self, builder: &mut RuntimeBuilder) {
        builder
            .add_plugin(SleepService)
            .add_plugin(TimerFactory)
            .add_plugin(StopwatchFactory);
    }
}

/// Receives a single floating-point number as a request, waits the value of
/// the number in seconds, then responds with an empty message.
pub struct SleepService;

#[async_trait]
impl SinkProcess for SleepService {
    type Message = f32;

    async fn on_message<'a>(&'a mut self, message: MessageInfo<'a, Self::Message>) {
        let Some(reply) = message.caps.first() else {
            debug!("Sleep request has no reply address");
            return;
        };

        let duration = Duration::from_secs_f32(message.data);
        let reply = reply.to_owned();
        let post = message.runtime.post.to_owned();

        // spawn thread to reply so that we can receive more sleep requests
        tokio::spawn(async move {
            tokio::time::sleep(duration).await;

            let table = Table::new(post);
            let reply_handle = table.import_owned(reply).unwrap();
            let reply_cap = table.wrap_handle(reply_handle).unwrap();
            reply_cap.send(&[], &[]).await.unwrap();
        });
    }
}

impl ServiceRunner for SleepService {
    const NAME: &'static str = "hearth.Sleep";

    fn get_process_metadata() -> ProcessMetadata {
        cargo_process_metadata!()
    }
}

/// Responds to empty request messages with a capability to a new instance of
/// a [Timer].
pub struct TimerFactory;

#[async_trait]
impl RequestResponseProcess for TimerFactory {
    type Request = ();
    type Response = ();

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, Self::Request>,
    ) -> ResponseInfo<'a, Self::Response> {
        let mut meta = cargo_process_metadata!();
        meta.name = Some("Timer".to_string());

        let last_request = Instant::now();
        let timer = Timer { last_request };
        let child = request.spawn(meta, timer);

        ResponseInfo {
            data: (),
            caps: vec![child],
        }
    }
}

impl ServiceRunner for TimerFactory {
    const NAME: &'static str = "hearth.TimerFactory";

    fn get_process_metadata() -> ProcessMetadata {
        cargo_process_metadata!()
    }
}

/// Waits a given interval beginning precisely from the end of the last wait.
///
/// Receives a floating-point number as a request, waits that number in seconds,
/// then sends back an empty message as response.
///
/// A timer's wait begins at the end of its last wait as opposed to when it
/// recievives a request. This is an improvement over [SleepService] as timers
/// function entirely on their internal clock and thus eliminate potential
/// round-trip time between requests and responses accumulating over multiple
/// sleep requests.
pub struct Timer {
    last_request: Instant,
}

#[async_trait]
impl RequestResponseProcess for Timer {
    type Request = f32;
    type Response = ();

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, Self::Request>,
    ) -> ResponseInfo<'a, Self::Response> {
        let duration = Duration::from_secs_f32(request.data);
        self.last_request += duration;
        tokio::time::sleep_until(self.last_request).await;

        ResponseInfo {
            data: (),
            caps: vec![],
        }
    }
}

/// Responds to empty request messages with a capability to a new instance of
/// a [Stopwatch].
pub struct StopwatchFactory;

#[async_trait]
impl RequestResponseProcess for StopwatchFactory {
    type Request = ();
    type Response = ();

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, Self::Request>,
    ) -> ResponseInfo<'a, Self::Response> {
        let mut meta = cargo_process_metadata!();
        meta.name = Some("Stopwatch".to_string());

        let last_request = Instant::now();
        let timer = Stopwatch { last_request };
        let child = request.spawn(meta, timer);

        ResponseInfo {
            data: (),
            caps: vec![child],
        }
    }
}

impl ServiceRunner for StopwatchFactory {
    const NAME: &'static str = "hearth.StopwatchFactory";

    fn get_process_metadata() -> ProcessMetadata {
        cargo_process_metadata!()
    }
}

/// Responds to empty request messages with a floating-point number that
/// represents the time in seconds since the last request.
pub struct Stopwatch {
    last_request: Instant,
}

#[async_trait]
impl RequestResponseProcess for Stopwatch {
    type Request = ();
    type Response = f32;

    async fn on_request<'a>(
        &'a mut self,
        _request: &mut RequestInfo<'a, Self::Request>,
    ) -> ResponseInfo<'a, Self::Response> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_request).as_secs_f32();
        self.last_request = now;

        ResponseInfo {
            data: elapsed,
            caps: vec![],
        }
    }
}
