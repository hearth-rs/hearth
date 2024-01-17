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

use std::{any::type_name, collections::HashMap, fmt::Debug, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use flue::{CapabilityHandle, CapabilityRef, OwnedTableSignal, Permissions, PostOffice, Table};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, trace};

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

/// An interface trait for data passed as context to process runners.
pub trait RunnerContext<'a> {
    /// Retrieves the inner process that this process runner has access to.
    fn get_process(&self) -> &'a Process;

    /// Retrieves the runtime that this process runner is a component of.
    fn get_runtime(&self) -> &'a Arc<Runtime>;

    /// Spawns a child process, executes it using the given process runner,
    /// and returns a capability to its parent mailbox within this runners'
    /// table.
    fn spawn(
        &self,
        meta: ProcessMetadata,
        runner: impl ProcessRunner + 'static,
    ) -> CapabilityRef<'a> {
        let label = meta.name.clone().unwrap_or("<no name>".to_string());
        let runtime = self.get_runtime().to_owned();
        let child = runtime.process_factory.spawn(meta);
        let perms = Permissions::all();

        let child_cap = child
            .borrow_parent()
            .export_to(perms, self.get_process().borrow_table())
            .unwrap();

        tokio::spawn(async move {
            runner.run(label, runtime, &child).await;
        });

        child_cap
    }
}

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

impl<'a, T> RunnerContext<'a> for MessageInfo<'a, T> {
    fn get_process(&self) -> &'a Process {
        self.process
    }

    fn get_runtime(&self) -> &'a Arc<Runtime> {
        self.runtime
    }
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

impl<'a, T> RunnerContext<'a> for RequestInfo<'a, T> {
    fn get_process(&self) -> &'a Process {
        self.process
    }

    fn get_runtime(&self) -> &'a Arc<Runtime> {
        self.runtime
    }
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
pub trait SinkProcess: Send {
    /// The deserializeable data type to be received.
    type Message: for<'a> Deserialize<'a> + Send + Debug;

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
pub trait RequestResponseProcess: Send {
    type Request: for<'a> Deserialize<'a> + Send + Debug;
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
    T: ServiceRunner + 'static,
{
    fn finalize(self, builder: &mut RuntimeBuilder) {
        let name = Self::NAME.to_string();
        let mut meta = Self::get_process_metadata();
        meta.name = Some(name.clone());
        builder.add_service(name, meta, self);
    }
}

/// A shared utility struct for publishing event messages of type `T` to a
/// dynamic list of subscribers.
pub struct PubSub<T> {
    table: Table,

    /// A mutex-locked set of subscribers. Each entry maps a zero permission
    /// capability as the index to a send-only capability for notifying.
    subscribers: Mutex<HashMap<CapabilityHandle, CapabilityHandle>>,

    /// We don't actually carry `T` in this struct, so we need to use it in a
    /// `PhantomData` so that the Rust compiler won't yell at us.
    _phantom: PhantomData<T>,
}

impl<T: Serialize> PubSub<T> {
    /// Create a new pub-sub structure.
    pub fn new(post: Arc<PostOffice>) -> Self {
        Self {
            table: Table::new(post),
            subscribers: Default::default(),
            _phantom: PhantomData,
        }
    }

    /// Adds a subscriber. Does nothing if the capability is already subscribed.
    ///
    /// The given capability can be from any table.
    ///
    /// Logs an error and doesn't subscribe if the cap doesn't have the send
    /// perm.
    pub fn subscribe(&self, cap: CapabilityRef) {
        // ensure that we can store a send cap
        if !cap.get_permissions().contains(Permissions::SEND) {
            let name = std::any::type_name::<T>();
            error!("Capability given to {} pubsub doesn't permit send", name);
            return;
        }

        let cap = self.table.import_ref(cap).unwrap();
        let key = cap.demote(Permissions::empty()).unwrap().into_handle();
        let val = cap.demote(Permissions::SEND).unwrap().into_handle();

        // lock the subscribers list
        let mut subs = self.subscribers.lock();

        // insert subscriber into map and catch existing entries
        if let Some(old_val) = subs.insert(key, val) {
            // manually decrement reference count for a duplicated subscriber
            self.table.dec_ref(key).unwrap();
            self.table.dec_ref(old_val).unwrap();
        }
    }

    /// Removes a subscriber. Does nothing if the cap is not already subscribed.
    ///
    /// The given capability can be from any table.
    pub fn unsubscribe(&self, cap: CapabilityRef) {
        let cap = self.table.import_ref(cap).unwrap();
        let key = cap.demote(Permissions::empty()).unwrap().into_handle();

        // lock the subscribers list
        let mut subs = self.subscribers.lock();

        // remove subscriber from map and catch the old lifetime
        if let Some(old_val) = subs.remove(&key) {
            // manually decrement reference count for removed subscriber
            self.table.dec_ref(key).unwrap();
            self.table.dec_ref(old_val).unwrap();
        }

        // decrement reference count for imported key
        self.table.dec_ref(key).unwrap();
    }

    /// Broadcasts an event to all current subscribers.
    pub async fn notify(&self, event: &T) {
        // attempt to serialize the event or gracefully fail
        let data = match serde_json::to_vec(event) {
            Ok(data) => data,
            Err(err) => {
                let name = std::any::type_name::<T>();
                error!("Failed to serialize {}: {:?}", name, err);
                return;
            }
        };

        // clone subscribers so that we can release the mutex during async
        let subscribers: Vec<_> = self
            .subscribers
            .lock()
            .values()
            .map(|handle| {
                // own handle while sending
                self.table.inc_ref(*handle).unwrap();
                *handle
            })
            .collect();

        // send the event to all subscribers
        for cap in subscribers {
            // send event
            self.table.send(cap, &data, &[]).await.unwrap();

            // free handle
            self.table.dec_ref(cap).unwrap();
        }
    }
}
