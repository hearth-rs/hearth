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

//! Process interface, store, messages, and related resources.
//!
//! To create a process store from scratch, call [ProcessStoreImpl::new]. This
//! is called automatically as part of runtime startup. Then, implement the
//! [Process] trait for process types, and spawn instances of those types in
//! a store with [ProcessStoreImpl::spawn].
//!
//! [ProcessStoreImpl] implements the [ProcessStore] RPC trait, which provides
//! access to the store to other network peers or IPC daemons.

use std::fmt::{Display, Formatter};
use std::sync::{Arc, Weak};

use hearth_rpc::remoc::robs::hash_map::HashMapSubscription;
use hearth_rpc::remoc::rtc::ServerShared;
use hearth_rpc::{Message as RpcMessage, *};
use hearth_types::*;
use remoc::rch::{mpsc as remoc_mpsc, watch as remoc_watch};
use remoc::robs::hash_map::ObservableHashMap;
use remoc::robs::list::{ListSubscription, ObservableList, ObservableListDistributor};
use remoc::rtc::async_trait;
use slab::Slab;
use tokio::sync::{mpsc, watch, RwLock};
use tracing::{debug, error, info, trace};

/// A send error that may occur when sending a message from one process to
/// another.
#[derive(Clone, Debug)]
pub enum SendError {
    /// The destination process ID was not found.
    ProcessNotFound(ProcessId),

    /// There was an error while sending over a Remoc channel.
    RemocSendError(remoc_mpsc::SendError<Message>),
}

impl From<remoc_mpsc::SendError<Message>> for SendError {
    fn from(err: remoc_mpsc::SendError<Message>) -> Self {
        SendError::RemocSendError(err)
    }
}

impl Display for SendError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::ProcessNotFound(pid) => write!(fmt, "process {:?} not found", pid),
            SendError::RemocSendError(err) => err.fmt(fmt),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SendError::ProcessNotFound(_) => None,
            SendError::RemocSendError(err) => Some(err),
        }
    }
}

/// An interface trait for processes.
///
/// To create a new type of process, this trait may be implemented. Then, an
/// instance of that type may passed to [ProcessStoreImpl::spawn] to spawn the
/// process and gain access to a process's functionality.
#[async_trait]
pub trait Process: Send + Sync + 'static {
    /// Returns the [ProcessInfo] for this process.
    ///
    /// Called once during spawning.
    fn get_info(&self) -> ProcessInfo;

    /// Runs this process using a [ProcessContext] allocated from the store.
    async fn run(&mut self, ctx: ProcessContext);
}

/// A single message sent from one process to another.
#[derive(Clone, Debug)]
pub struct Message {
    /// The ID of the process that sent this message.
    ///
    /// This may be used as a return address for reply messages.
    pub sender: ProcessId,

    /// The message's data.
    pub data: Vec<u8>,
}

/// The full set of resources provided for a single process.
///
/// Process implementations may use this struct to perform all of their duties
/// as a process, including sending and receiving messages, accessing the
/// full runtime, and logging events to this process's log.
pub struct ProcessContext {
    /// The ID of this process.
    pid: ProcessId,

    /// The store that contains this process.
    process_store: Arc<ProcessStoreImpl>,

    /// A queue of all messages sent to this process.
    mailbox: mpsc::Receiver<Message>,

    /// True when this process is not dead.
    is_alive: watch::Receiver<bool>,

    /// Sender to set [is_alive] manually.
    is_alive_tx: Arc<watch::Sender<bool>>,

    /// Channel to send IDs of killed processes to.
    on_kill_tx: mpsc::UnboundedSender<LocalProcessId>,

    /// Observable log for this process's log events.
    log: ObservableList<ProcessLogEvent>,

    /// A sender to this process's number of warning logs.
    warning_num_tx: remoc_watch::Sender<u32>,

    /// A sender to this process's number of error logs.
    error_num_tx: remoc_watch::Sender<u32>,

    /// A sender to this process's total number of log events.
    log_num_tx: remoc_watch::Sender<u32>,
}

impl Drop for ProcessContext {
    fn drop(&mut self) {
        // kill this process if it hasn't been already
        if self.is_alive() {
            self.kill();
        }
    }
}

impl ProcessContext {
    /// Gets the [ProcessId] for this process.
    pub fn get_pid(&self) -> ProcessId {
        self.pid
    }

    /// Gets this context's [ProcessStoreImpl].
    pub fn get_process_store(&self) -> &Arc<ProcessStoreImpl> {
        &self.process_store
    }

    /// Returns true when this process is still alive.
    pub fn is_alive(&self) -> bool {
        *self.is_alive.borrow()
    }

    /// Waits for this process to complete.
    ///
    /// Can be combined with `tokio::select` to wait on another future or quit
    /// when this process is killed. Useful for signalling async processes to
    /// exit their event loop.
    pub async fn join(&mut self) {
        while self.is_alive() {
            let _ = self.is_alive.changed().await;
        }
    }

    /// Sends a message to another process.
    pub async fn send_message(&self, dst: ProcessId, data: Vec<u8>) -> Result<(), SendError> {
        let (peer, local_dst) = dst.split();

        let msg = Message {
            sender: self.pid,
            data,
        };

        if peer == self.pid.split().0 {
            self.process_store.send_message(local_dst, msg).await
        } else {
            error!("Remote process message sending is unimplemented");
            Err(remoc_mpsc::SendError::RemoteForward.into())
        }
    }

    /// Receives a single message to this process.
    ///
    /// Returns `None` if this process is dead.
    pub async fn recv(&mut self) -> Option<Message> {
        self.mailbox.recv().await
    }

    /// Kills this process.
    ///
    /// Does nothing if it's already been killed.
    pub fn kill(&mut self) {
        if self.is_alive_tx.send_replace(false) {
            let (_peer, local_pid) = self.pid.split();
            let _ = self.on_kill_tx.send(local_pid); // ignore result; not responsible for killing if the receiver's store is unavailable
        }
    }

    /// Adds a log event to this process's log.
    pub fn log(&mut self, event: ProcessLogEvent) {
        // helper function for incrementing watched counter
        let inc_num = |watch: &mut remoc_watch::Sender<u32>| {
            watch.send_modify(|i| *i += 1);
        };

        // update level-specific log event counters
        match event.level {
            ProcessLogLevel::Warning => inc_num(&mut self.warning_num_tx),
            ProcessLogLevel::Error => inc_num(&mut self.error_num_tx),
            _ => {}
        }

        // always increment the total log event counter
        inc_num(&mut self.log_num_tx);

        // actually push the log event
        self.log.push(event);
    }
}

struct RemoteProcess {
    info: ProcessInfo,
    mailbox: remoc_mpsc::Sender<RpcMessage>,
    outgoing: remoc_mpsc::Receiver<RpcMessage>,
    is_alive: remoc_watch::Sender<bool>,
    log: remoc_mpsc::Receiver<ProcessLogEvent>,
}

#[async_trait]
impl Process for RemoteProcess {
    fn get_info(&self) -> ProcessInfo {
        self.info.clone()
    }

    async fn run(&mut self, mut ctx: ProcessContext) {
        while ctx.is_alive() {
            tokio::select! {
                msg = ctx.mailbox.recv() => self.on_recv(&mut ctx, msg).await,
                _ = ctx.is_alive.changed() => self.on_is_alive(&mut ctx).await,
                msg = self.outgoing.recv() => self.on_outgoing(&mut ctx, msg).await,
                log = self.log.recv() => self.on_log(&mut ctx, log).await,
            }
        }
    }
}

impl RemoteProcess {
    async fn on_recv(&mut self, ctx: &mut ProcessContext, msg: Option<Message>) {
        let msg = match msg {
            Some(msg) => msg,
            None => return,
        };

        let msg = RpcMessage {
            pid: msg.sender,
            data: msg.data,
        };

        if self.mailbox.send(msg).await.is_err() {
            // SendErrors are always final
            debug!("RemoteProcess channel hung up (mailbox SendError)");
            self.on_kill(ctx); // remote hung up
        }
    }

    async fn on_is_alive(&mut self, ctx: &mut ProcessContext) {
        if !ctx.is_alive() {
            let _ = self.is_alive.send(false); // ignore result; no biggie if the remote hangs up
        }
    }

    async fn on_outgoing(
        &mut self,
        ctx: &mut ProcessContext,
        msg: Result<Option<RpcMessage>, remoc_mpsc::RecvError>,
    ) {
        if let Some(msg) = self.handle_recv_result(ctx, msg) {
            // TODO communicate send errors back to process base
            let _ = ctx.send_message(msg.pid, msg.data).await;
        }
    }

    async fn on_log(
        &mut self,
        ctx: &mut ProcessContext,
        log: Result<Option<ProcessLogEvent>, remoc_mpsc::RecvError>,
    ) {
        if let Some(log) = self.handle_recv_result(ctx, log) {
            ctx.log(log);
        }
    }

    fn handle_recv_result<T>(
        &mut self,
        ctx: &mut ProcessContext,
        result: Result<Option<T>, remoc_mpsc::RecvError>,
    ) -> Option<T> {
        match result {
            Ok(Some(val)) => Some(val),
            Ok(None) => {
                debug!("RemoteProcess channel hung up (end of channel)");
                self.on_kill(ctx); // remote hung up
                None
            }
            Err(err) if err.is_final() => {
                debug!("RemoteProcess channel hung up (RecvError::is_final() == true)");
                self.on_kill(ctx); // remote hung up
                None
            }
            Err(err) => {
                error!("RemoteProcess recv() error: {:?}", err);
                None
            }
        }
    }

    fn on_kill(&mut self, ctx: &mut ProcessContext) {
        debug!("RemoteProcess has been killed");
        let _ = self.is_alive.send(false);
        ctx.kill();
    }
}

#[derive(Clone)]
struct ProcessWrapper {
    mailbox_tx: mpsc::Sender<Message>,
    log_distributor: ObservableListDistributor<ProcessLogEvent>,
    is_alive_tx: Arc<watch::Sender<bool>>,
}

struct ProcessApiImpl {
    pid: LocalProcessId,
    log_distributor: ObservableListDistributor<ProcessLogEvent>,
    is_alive_tx: Arc<watch::Sender<bool>>,
    on_kill_tx: mpsc::UnboundedSender<LocalProcessId>,
}

#[async_trait]
impl ProcessApi for ProcessApiImpl {
    async fn is_alive(&self) -> CallResult<bool> {
        Ok(*self.is_alive_tx.borrow())
    }

    async fn kill(&self) -> CallResult<()> {
        if self.is_alive_tx.send_replace(false) {
            let _ = self.on_kill_tx.send(self.pid);
        }

        Ok(())
    }

    async fn follow_log(&self) -> CallResult<ListSubscription<ProcessLogEvent>> {
        Ok(self.log_distributor.subscribe())
    }
}

#[derive(Default)]
struct ProcessStoreInner {
    services: ObservableHashMap<String, LocalProcessId>,
    process_statuses: ObservableHashMap<LocalProcessId, ProcessStatus>,
    processes: Slab<ProcessWrapper>,
}

pub struct ProcessStoreImpl {
    inner: RwLock<ProcessStoreInner>,
    this_peer: PeerId,
    on_kill_tx: mpsc::UnboundedSender<LocalProcessId>,
}

impl ProcessStoreImpl {
    pub fn new(this_peer: PeerId) -> Arc<Self> {
        let (on_kill_tx, on_kill_rx) = mpsc::unbounded_channel();

        let store = Arc::new(Self {
            inner: Default::default(),
            this_peer,
            on_kill_tx,
        });

        Self::listen_for_kills(Arc::downgrade(&store), on_kill_rx);

        store
    }

    fn listen_for_kills(store: Weak<Self>, mut on_kill: mpsc::UnboundedReceiver<LocalProcessId>) {
        debug!("Spawning on_kill listener thread");
        tokio::spawn(async move {
            loop {
                trace!("Listening for on_kill message");

                let pid = match on_kill.recv().await {
                    Some(pid) => pid,
                    None => {
                        debug!("All on_kill senders closed; exiting");
                        break;
                    }
                };

                trace!("Removing {:?} from store", pid);

                if let Some(store) = store.upgrade() {
                    let mut inner = store.inner.write().await;
                    inner.process_statuses.remove(&pid);

                    // if there is actually a process with this ID, notify its context that it's dead
                    if let Some(wrapper) = inner.processes.try_remove(pid.0 as usize) {
                        let _ = wrapper.is_alive_tx.send(false); // ignore error; not our problem if the remote has hung up
                    } else {
                        // double-removal race conditions are bugs
                        error!("Attempted to kill dead PID {}", pid.0);
                    }

                    // TODO reverse lookup?
                    let service = inner
                        .services
                        .iter()
                        .find(|(_name, service_pid)| **service_pid == pid)
                        .map(|(name, _pid)| name.to_owned());

                    if let Some(service) = service {
                        debug!("Removing {} service", service);
                        inner.services.remove(&service);
                    }
                } else {
                    debug!("All process store references dropped; exiting");
                    break;
                }
            }
        });
    }

    async fn send_message(&self, dst: LocalProcessId, msg: Message) -> Result<(), SendError> {
        let full_dst = ProcessId::from_peer_process(self.this_peer, dst);
        let sender = if let Some(wrapper) = self.inner.read().await.processes.get(dst.0 as usize) {
            wrapper.mailbox_tx.clone()
        } else {
            return Err(SendError::ProcessNotFound(full_dst));
        };

        match sender.send(msg).await {
            Ok(()) => Ok(()),
            Err(_err) => {
                error!("Process wrapper was fetched but process mailbox hung up");
                Err(SendError::ProcessNotFound(full_dst))
            }
        }
    }

    /// Allocates a process and its [ProcessContext].
    ///
    /// To actually spawn a [Process] implementation, use [Self::spawn]
    /// instead. This function only allocates the context for a process
    /// without running it.
    pub async fn spawn_context(self: &Arc<Self>, info: ProcessInfo) -> ProcessContext {
        let (mailbox_tx, mailbox) = mpsc::channel(1024);
        let (is_alive_tx, is_alive) = watch::channel(true);
        let is_alive_tx = Arc::new(is_alive_tx);
        let log = ObservableList::new();
        let mut store = self.inner.write().await;

        // this needs to be inside a block because entry doesn't implement the
        // Send trait and even though entry.insert() takes ownership of entry
        // the compiler still complains about entry maybe being used across
        // the await making this function's future non-Send
        let pid = {
            let entry = store.processes.vacant_entry();
            let pid: u32 = entry.key().try_into().expect("PID integer overflow");
            let pid = LocalProcessId(pid);

            entry.insert(ProcessWrapper {
                mailbox_tx,
                log_distributor: log.distributor(),
                is_alive_tx: is_alive_tx.clone(),
            });

            debug!("Allocated {:?}", pid);

            pid
        };

        let (warning_num_tx, warning_num) = remoc_watch::channel(0);
        let (error_num_tx, error_num) = remoc_watch::channel(0);
        let (log_num_tx, log_num) = remoc_watch::channel(0);

        let status = ProcessStatus {
            warning_num,
            error_num,
            log_num,
            info,
        };

        store.process_statuses.insert(pid, status);

        ProcessContext {
            pid: ProcessId::from_peer_process(self.this_peer, pid),
            on_kill_tx: self.on_kill_tx.clone(),
            process_store: self.to_owned(),
            mailbox,
            is_alive,
            is_alive_tx,
            log,
            warning_num_tx,
            error_num_tx,
            log_num_tx,
        }
    }

    /// Spawns a process.
    pub async fn spawn(self: &Arc<Self>, mut process: impl Process) -> LocalProcessId {
        let info = process.get_info();
        let ctx = self.spawn_context(info).await;
        let (_peer, pid) = ctx.pid.split();

        tokio::spawn(async move {
            process.run(ctx).await;
        });

        pid
    }
}

#[async_trait]
impl ProcessStore for ProcessStoreImpl {
    async fn print_hello_world(&self) -> CallResult<()> {
        info!("Hello, world!");
        Ok(())
    }

    async fn find_process(&self, pid: LocalProcessId) -> ResourceResult<ProcessApiClient> {
        match self.inner.read().await.processes.get(pid.0 as usize) {
            None => Err(ResourceError::Unavailable),
            Some(wrapper) => {
                let api = Arc::new(ProcessApiImpl {
                    pid,
                    log_distributor: wrapper.log_distributor.clone(),
                    is_alive_tx: wrapper.is_alive_tx.clone(),
                    on_kill_tx: self.on_kill_tx.clone(),
                });

                let (server, client) =
                    ProcessApiServerShared::<_, remoc::codec::Default>::new(api, 128);

                tokio::spawn(async move {
                    server.serve(true).await;
                });

                Ok(client)
            }
        }
    }

    async fn register_service(&self, pid: LocalProcessId, name: String) -> ResourceResult<()> {
        debug!("Registering service '{}' to {:?}", name, pid);
        let mut store = self.inner.write().await;
        if !store.processes.contains(pid.0 as usize) {
            debug!("Invalid local process ID");
            return Err(ResourceError::Unavailable);
        } else if store.services.contains_key(&name) {
            debug!("Service name is taken");
            Err(ResourceError::BadParams)
        } else {
            store.services.insert(name, pid);
            Ok(())
        }
    }

    async fn deregister_service(&self, name: String) -> ResourceResult<()> {
        debug!("Deregistering service '{}'", name);
        if self.inner.write().await.services.remove(&name).is_none() {
            Err(ResourceError::Unavailable)
        } else {
            Ok(())
        }
    }

    async fn follow_process_list(
        &self,
    ) -> CallResult<HashMapSubscription<LocalProcessId, ProcessStatus>> {
        Ok(self.inner.read().await.process_statuses.subscribe(1024))
    }

    async fn follow_service_list(&self) -> CallResult<HashMapSubscription<String, LocalProcessId>> {
        Ok(self.inner.read().await.services.subscribe(1024))
    }
}

pub struct ProcessFactoryImpl {
    process_store: Arc<ProcessStoreImpl>,
}

#[async_trait]
impl ProcessFactory for ProcessFactoryImpl {
    async fn spawn(&self, process: ProcessBase) -> CallResult<ProcessOffer> {
        let (outgoing_tx, outgoing) = remoc_mpsc::channel(1024);

        let process = RemoteProcess {
            info: process.info,
            outgoing,
            mailbox: process.mailbox,
            is_alive: process.is_alive,
            log: process.log,
        };

        let pid = self.process_store.spawn(process).await;

        Ok(ProcessOffer {
            outgoing: outgoing_tx,
            pid,
        })
    }
}

impl ProcessFactoryImpl {
    pub fn new(process_store: Arc<ProcessStoreImpl>) -> Self {
        Self { process_store }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A useless process with no significant info that does nothing.
    struct DummyProcess;

    #[async_trait]
    impl Process for DummyProcess {
        fn get_info(&self) -> ProcessInfo {
            ProcessInfo {}
        }

        async fn run(&mut self, _ctx: ProcessContext) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    #[tokio::test]
    async fn register_service() {
        let store = ProcessStoreImpl::new(PeerId(42));
        let pid = store.spawn(DummyProcess).await;
        store.register_service(pid, "test".into()).await.unwrap();
    }
}
