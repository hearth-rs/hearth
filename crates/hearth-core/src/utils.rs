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

use std::{any::type_name, fmt::Debug, sync::Arc};

use async_trait::async_trait;
use flue::{CapabilityRef, OwnedTableSignal};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::{
    process::{Process, ProcessMetadata},
    runtime::{Plugin, Runtime, RuntimeBuilder},
};

/// Helper macro to initialize [ProcessMetadata] using Cargo environment variables.
///
/// This macro initializes these [ProcessMetadata] fields with `CARGO_PKG_*`
/// environment variables:
/// - `authors`: `CARGO_PKG_AUTHORS`
/// - `repository`: `CARGO_PKG_REPOSITORY`
/// - `homepage`: `CARGO_PKG_HOMEPAGE`
/// - `license`: `CARGO_PKG_LICENSE`
#[macro_export]
macro_rules! cargo_process_metadata {
    () => {{
        let mut meta = ProcessMetadata::default();

        // returns `None` if the string is empty, or `Some(str)` otherwise.
        let some_or_empty = |str: &str| {
            if str.is_empty() {
                None
            } else {
                Some(str.to_string())
            }
        };

        meta.authors = some_or_empty(env!("CARGO_PKG_AUTHORS"))
            .map(|authors| authors.split(':').map(ToString::to_string).collect());

        meta.repository = some_or_empty(env!("CARGO_PKG_REPOSITORY"));
        meta.homepage = some_or_empty(env!("CARGO_PKG_HOMEPAGE"));
        meta.license = some_or_empty(env!("CARGO_PKG_LICENSE"));
        meta
    }};
}

// export the macro so we can use it in other modules in this crate
pub(crate) use cargo_process_metadata;

/// Context for an incoming message in [SinkProcess].
pub struct MessageInfo<'a, T> {
    /// This process's label.
    pub label: &'a str,

    /// The [Process] that has received this message.
    pub process: &'a Process,

    /// A handle to the [Runtime] this process is running in.
    pub runtime: &'a Arc<Runtime>,

    /// The deserialized data of the message's contents.
    pub data: T,

    /// The capabilities from this message.
    pub caps: &'a [CapabilityRef<'a>],
}

/// Context for an incoming message in [RequestResponseProcess].
pub struct RequestInfo<'a, T> {
    /// This process's label.
    pub label: &'a str,

    /// The [Process] that has received this message.
    pub process: &'a Process,

    /// The capability handle of the first capability from the message.
    pub reply: CapabilityRef<'a>,

    /// The rest of the capabilities from the message.
    pub cap_args: &'a [CapabilityRef<'a>],

    /// A handle to the [Runtime] this process is running in.
    pub runtime: &'a Arc<Runtime>,

    /// The deserialized data of the message's contents.
    pub data: T,
}

pub struct ResponseInfo<'a, T>
where
    T: Send,
{
    pub data: T,
    pub caps: Vec<CapabilityRef<'a>>,
}

impl<'a, T> From<T> for ResponseInfo<'a, T>
where
    T: Send,
{
    fn from(data: T) -> Self {
        Self { data, caps: vec![] }
    }
}

impl<'a, O, E> From<E> for ResponseInfo<'a, Result<O, E>>
where
    O: Send,
    E: Send,
{
    fn from(err: E) -> Self {
        Self {
            data: Err(err),
            caps: vec![],
        }
    }
}

/// A trait for types that implement process behavior.
#[async_trait]
pub trait ProcessRunner: Send {
    /// Executes this process.
    ///
    /// Takes ownership of this object and provides a dev-facing label, a handle
    /// to the runtime, and an existing [Process] instance as context.
    async fn run(mut self, label: String, runtime: Arc<Runtime>, ctx: &Process);
}

/// A trait for process runners that continuously receive JSON-formatted messages of a single type.
///
/// This trait has a blanket implementation for [ProcessRunner] that loops and
/// receives new messages of the given data type, and calls [Self::on_message]
/// with a [RequestInfo].
#[async_trait]
pub trait SinkProcess: Send + Sync {
    /// The deserializeable data type to be received.
    type Message: for<'a> Deserialize<'a> + Send + Sync + Debug;

    /// A callback to call when messages are received by this process.
    async fn on_message<'a>(&'a mut self, message: MessageInfo<'a, Self::Message>);

    /// A callback to call when a down signal is received by this process.
    ///
    /// The capability passed is the capability in the down signal; a version
    /// of the monitored capability with no permissions.
    async fn on_down<'a>(&'a mut self, _cap: CapabilityRef<'a>) {}
}

#[async_trait]
impl<T> ProcessRunner for T
where
    T: SinkProcess,
{
    async fn run(mut self, label: String, runtime: Arc<Runtime>, ctx: &Process) {
        loop {
            let recv = ctx.borrow_parent().recv_owned().await;

            use OwnedTableSignal::*;
            match recv {
                Some(Message { data, caps }) => {
                    let data: T::Message = match serde_json::from_slice(&data) {
                        Ok(request) => request,
                        Err(err) => {
                            // TODO make this a process log
                            debug!("Failed to parse {}: {:?}", type_name::<T::Message>(), err);
                            continue;
                        }
                    };

                    trace!("{:?} received {:?}", label, data);

                    self.on_message(MessageInfo {
                        label: &label,
                        process: ctx,
                        runtime: &runtime,
                        data,
                        caps: &caps,
                    })
                    .await;

                    trace!("{:?} finished processing message", label);
                }
                Some(Down { handle }) => {
                    self.on_down(handle).await;
                }
                None => break, // killed; quit
            }
        }
    }
}

#[async_trait]
pub trait RequestResponseProcess: Send + Sync {
    type Request: for<'a> Deserialize<'a> + Send + Sync + Debug;
    type Response: Serialize + Send + Debug;

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, Self::Request>,
    ) -> ResponseInfo<'a, Self::Response>;

    /// A callback to call when a down signal is received by this process.
    ///
    /// The capability passed is the capability in the down signal; a version
    /// of the monitored capability with no permissions.
    async fn on_down<'a>(&'a mut self, _cap: CapabilityRef<'a>) {}
}

#[async_trait]
impl<T> SinkProcess for T
where
    T: RequestResponseProcess,
{
    type Message = T::Request;

    async fn on_message<'a>(&'a mut self, message: MessageInfo<'a, Self::Message>) {
        let Some(reply) = message.caps.first().cloned() else {
            debug!("Request to {:?} has no reply address", message.label);
            return;
        };

        let mut request = RequestInfo {
            label: message.label,
            process: message.process,
            reply: reply.clone(),
            cap_args: &message.caps[1..],
            runtime: message.runtime,
            data: message.data,
        };

        let response = self.on_request(&mut request).await;
        let data = serde_json::to_vec(&response.data).unwrap();
        let caps: Vec<_> = response.caps.iter().collect();
        let result = reply.send(&data, &caps).await;

        if let Err(err) = result {
            debug!("{:?} reply error: {:?}", message.label, err);
        }
    }

    async fn on_down<'a>(&'a mut self, cap: CapabilityRef<'a>) {
        // clarify trait so we don't make this function recursive
        <T as RequestResponseProcess>::on_down(self, cap).await;
    }
}

pub trait ServiceRunner: ProcessRunner {
    const NAME: &'static str;

    /// Gets the [ProcessMetadata] for this service.
    ///
    /// The `name` field of this struct is overridden by [Self::NAME].
    fn get_process_metadata() -> ProcessMetadata;
}

impl<T> Plugin for T
where
    T: ServiceRunner + Send + Sync + 'static,
{
    fn finalize(self, builder: &mut RuntimeBuilder) {
        let name = Self::NAME.to_string();
        let mut meta = Self::get_process_metadata();
        meta.name = Some(name.clone());
        builder.add_service(name, meta, self);
    }
}
