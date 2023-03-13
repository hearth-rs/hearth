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

use hearth_types::*;

use remoc::rch::{mpsc, watch};
use remoc::robj::lazy_blob::LazyBlob;
use remoc::robs::hash_map::HashMapSubscription;
use remoc::robs::list::ListSubscription;
use remoc::rtc::{remote, CallError};
use serde::{Deserialize, Serialize};

pub use hearth_types;
pub use remoc;

pub type CallResult<T> = Result<T, CallError>;

/// Wrapper around a [CallError] for requests involving resources.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ResourceError {
    /// A resource being referenced is unavailable.
    Unavailable,

    /// A resource was referenced in the context of invalid parameters.
    BadParams,

    /// There was a Remoc [CallError],
    CallError(CallError),
}

impl From<CallError> for ResourceError {
    fn from(err: CallError) -> Self {
        ResourceError::CallError(err)
    }
}

/// See [ResourceError] for more info.
pub type ResourceResult<T> = Result<T, ResourceError>;

/// An interface for acquiring access to the other peers on the network.
#[remote]
pub trait PeerProvider {
    /// Retrieves the [PeerApi] of a peer by its ID, if there is a peer with that ID.
    async fn find_peer(&self, id: PeerId) -> ResourceResult<PeerApiClient>;

    /// Subscribes to the list of peers in the space.
    async fn follow_peer_list(&self) -> CallResult<HashMapSubscription<PeerId, PeerInfo>>;
}

/// The initial data sent from server to client when a client connects.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerOffer {
    /// A remote [PeerProvider] for accessing the rest of the peers on the network.
    pub peer_provider: PeerProviderClient,

    /// The new [PeerId] for this client.
    pub new_id: PeerId,
}

/// The initial data sent from server to client after a client receives [ServerOffer].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClientOffer {
    /// The remote [PeerApi] of this client.
    pub peer_api: PeerApiClient,
}

/// The data sent from an IPC daemon to a client on connection.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonOffer {
    /// A [PeerProvider] to all peers on the daemon's network.
    pub peer_provider: PeerProviderClient,

    /// The ID of this daemon's peer.
    pub peer_id: PeerId,

    /// The [ProcessFactory] on this daemon. Can be used to spawn
    /// out-of-runtime processes.
    pub process_factory: ProcessFactoryClient,
}

/// Top-level interface for a peer. Provides access to its metadata as well as
/// its lower-level interfaces.
///
/// This is an example of the [Service Locator design pattern](https://gameprogrammingpatterns.com/service-locator.html).
/// This is considered an anti-pattern by some because services acquired
/// through it cannot be easily tested. However, this is not an issue in this
/// usecase because all this interface provides access to are procedural client
/// implementations to the real remote implementation, which could be made
/// testable with mocks at no consequence on this interface.
#[remote]
pub trait PeerApi {
    /// Gets this peer's metadata.
    async fn get_info(&self) -> CallResult<PeerInfo>;

    /// Gets this peer's process store.
    async fn get_process_store(&self) -> CallResult<ProcessStoreClient>;

    /// Gets this peer's lump store.
    async fn get_lump_store(&self) -> CallResult<LumpStoreClient>;
}

/// A peer's metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerInfo {
    /// This peer's nickname, if it has one.
    pub nickname: Option<String>,
}

/// Interface to a peer's process store. This is where all the magic happens.
///
/// Note that all process IDs (PIDs) are *local* PIDs, not global PIDs, because
/// this store belongs to a specific peer.
#[remote]
pub trait ProcessStore {
    /// Placeholder function call for testing.
    async fn print_hello_world(&self) -> CallResult<()>;

    /// Retrieves the [ProcessApi] for a process.
    async fn find_process(&self, pid: LocalProcessId) -> ResourceResult<ProcessApiClient>;

    /// Registers a process as a named service.
    ///
    /// Returns [ResourceError::BadParams] if the service name is taken.
    async fn register_service(&self, pid: LocalProcessId, name: String) -> ResourceResult<()>;

    /// Deregisters a service.
    async fn deregister_service(&self, name: String) -> ResourceResult<()>;

    /// Subscribes to this store's process list.
    ///
    /// This list is updated live as processes are spawned, killed, or changed.
    async fn follow_process_list(
        &self,
    ) -> CallResult<HashMapSubscription<LocalProcessId, ProcessInfo>>;

    /// Subscribes to this store's service list.
    ///
    /// This list is updated live as services are registered and deregistered.
    async fn follow_service_list(&self) -> CallResult<HashMapSubscription<String, LocalProcessId>>;

    // TODO Lunatic Supervisor-like child API?
}

/// Spawning interface to a peer's process store for out-of-runtime processes.
#[remote]
pub trait ProcessFactory {
    /// Spawns a remote process.
    async fn spawn(&self, process: ProcessBase) -> CallResult<ProcessOffer>;
}

/// The result of [ProcessFactory::ProcessOffer]. Sent from the runtime to an
/// IPC client as part of the spawning of an out-of-runtime process.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcessOffer {
    /// A sender for outgoing process messages.
    ///
    /// The destination process ID for each message is passed through the
    /// [Message::pid] field.
    pub outgoing: mpsc::Sender<Message>,

    /// The new PID for this process.
    pub pid: LocalProcessId,
}

/// Interface to a single process.
///
/// All of these methods still function when the process is dead.
#[remote]
pub trait ProcessApi {
    /// Returns true if this process is still alive.
    async fn is_alive(&self) -> CallResult<bool>;

    /// Kills this process.
    async fn kill(&self) -> CallResult<()>;

    /// Subscribes to this process's log.
    ///
    /// Even if the process is dead, this will still return a full log history.
    async fn follow_log(&self) -> CallResult<ListSubscription<ProcessLogEvent>>;
}

/// Channels connected to the base functionality of a single process.
///
/// While [ProcessApi] defines an interface to interact with a running process,
/// this is instead an interface for implementing a process itself. This is
/// intended to be initialized by an IPC client for creating native IPC
/// processes that execute outside of the main Hearth runtime. Because only
/// processes can send messages to other processes, this is a necessary
/// prerequisite for IPC to interact with Hearth processes via messages.
#[derive(Deserialize, Serialize)]
pub struct ProcessBase {
    /// The info for this process.
    pub info: ProcessInfo,

    /// A sender to this process's mailbox.
    ///
    /// The `pid` field of each message represents the sender of the message.
    pub mailbox: mpsc::Sender<Message>,

    /// A watchable sender to this process's alive status.
    pub is_alive: watch::Sender<bool>,

    /// A receiver to this process's log.
    pub log: mpsc::Receiver<ProcessLogEvent>,
}

/// A single message between processes.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    /// The ID of the involved process. May be either the sender or receiver,
    /// depending on the context this struct is used in.
    pub pid: ProcessId,

    /// The data in this message.
    pub data: Vec<u8>,
}

/// Interface to a peer's local lumps.
#[remote]
pub trait LumpStore {
    /// Uploads a new lump to this store.
    ///
    /// If the uploading of the lump fails, this request will fail with
    /// [ResourceResult::Unavailable].
    ///
    /// Has an optional [LumpId] parameter to skip the uploading of a lump if
    /// the lump is already available. If an ID is provided but the data's
    /// hash does not match that ID, this request will fail with
    /// [ResourceResult::BadParams].
    async fn upload_lump(&self, id: Option<LumpId>, data: LazyBlob) -> ResourceResult<LumpId>;

    /// Downloads a lump from this store.
    async fn download_lump(&self, id: LumpId) -> ResourceResult<LazyBlob>;
}

/// Log event emitted by a process.
#[derive(Clone, Debug, Hash, Deserialize, Serialize)]
pub struct ProcessLogEvent {
    pub level: ProcessLogLevel,
    pub module: String,
    pub content: String,
    // TODO optional source code location?
    // TODO serializeable timestamp?
}

/// A process's metadata.
#[derive(Clone, Debug, Hash, Deserialize, Serialize)]
pub struct ProcessInfo {}
