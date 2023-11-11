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

use std::sync::Arc;

use anyhow::{anyhow, Result};
use hearth_core::anyhow::{self, bail, Context};
use hearth_core::asset::AssetLoader;
use hearth_core::flue::{CapabilityHandle, Mailbox, MailboxGroup, Permissions, Table, TableSignal};
use hearth_core::lump::{bytes::Bytes, LumpStoreImpl};
use hearth_core::process::{Process, ProcessLogEvent, ProcessMetadata};
use hearth_core::runtime::{Plugin, Runtime, RuntimeBuilder};
use hearth_core::{async_trait, hearth_types};
use hearth_core::{cargo_process_metadata, tokio, utils::*};
use hearth_macros::impl_wasm_linker;
use hearth_types::wasm::WasmSpawnInfo;
use hearth_types::{LumpId, SignalKind};
use hearth_wasm::{GuestMemory, WasmLinker};
use slab::Slab;
use tracing::{debug, error};
use wasmtime::{Caller, Config, Engine, Linker, Module, Store, UpdateDeadline};

/// Implements the `hearth::log` ABI module.
pub struct LogAbi {
    process: Arc<Process>,
}

#[impl_wasm_linker(module = "hearth::log")]
impl LogAbi {
    /// Logs an event for this process.
    ///
    /// Each argument corresponds to a field in [ProcessLogEvent].
    async fn log(
        &self,
        memory: GuestMemory<'_>,
        level: u32,
        module_ptr: u32,
        module_len: u32,
        content_ptr: u32,
        content_len: u32,
    ) -> Result<()> {
        let level = level
            .try_into()
            .map_err(|_| anyhow!("invalid log level constant {}", level))?;

        let event = ProcessLogEvent {
            level,
            module: memory.get_str(module_ptr, module_len)?.to_string(),
            content: memory.get_str(content_ptr, content_len)?.to_string(),
        };

        self.process.borrow_info().log_tx.send(event)?;

        Ok(())
    }
}

/// A script-local lump stored in [LumpAbi].
#[derive(Debug)]
pub struct LocalLump {
    pub id: LumpId,
    pub bytes: Bytes,
}

/// Implements the `hearth::lump` ABI module.
///
/// This works with two main data types: lump handles and lump ID pointers.
/// Handles are lumps that have been "loaded" into this process and can be
/// directly copied into guest memory. Lump ID pointers refer to a guest-side
/// [LumpId] data type that is directly read from and written to by the host.
#[derive(Debug)]
pub struct LumpAbi {
    pub lump_store: Arc<LumpStoreImpl>,
    pub lump_handles: Slab<LocalLump>,
    pub this_lump: LumpId,
}

#[impl_wasm_linker(module = "hearth::lump")]
impl LumpAbi {
    /// Retrieves the [LumpId] of the WebAssembly module lump of the currently
    /// running process. Writes the result into the guest memory at the given
    /// [LumpId] pointer.
    async fn this_lump(&self, memory: GuestMemory<'_>, id_ptr: u32) -> Result<()> {
        let id: &mut LumpId = memory.get_memory_ref(id_ptr)?;
        *id = self.this_lump;
        Ok(())
    }

    /// Load a lump from its [LumpId], retrieved from guest memory via pointer.
    ///
    /// Fails if the lump is not found in the lump store.
    async fn load_by_id(&mut self, memory: GuestMemory<'_>, id_ptr: u32) -> Result<u32> {
        let id: LumpId = *memory.get_memory_ref(id_ptr)?;
        let bytes = self
            .lump_store
            .get_lump(&id)
            .await
            .ok_or_else(|| anyhow!("couldn't find {:?} in lump store", id))?;
        Ok(self.lump_handles.insert(LocalLump { id, bytes }) as u32)
    }

    /// Loads a lump from guest memory.
    async fn load(&mut self, memory: GuestMemory<'_>, data_ptr: u32, data_len: u32) -> Result<u32> {
        let bytes: Bytes = memory.get_slice(data_ptr, data_len)?.to_vec().into();
        let id = self.lump_store.add_lump(bytes.clone()).await;
        let lump = LocalLump { id, bytes };
        let handle = self.lump_handles.insert(lump) as u32;
        Ok(handle)
    }

    /// Writes the [LumpId] of a loaded lump to guest memory via pointer.
    fn get_id(&self, memory: GuestMemory<'_>, handle: u32, id_ptr: u32) -> Result<()> {
        let lump = self.get_lump(handle)?;
        let id: &mut LumpId = memory.get_memory_ref(id_ptr)?;
        *id = lump.id;
        Ok(())
    }

    /// Gets the length of a loaded lump by handle.
    fn get_len(&self, handle: u32) -> Result<u32> {
        self.get_lump(handle).map(|lump| lump.bytes.len() as u32)
    }

    /// Copies the data of a loaded lump into guest memory by handle.
    ///
    /// The length required to copy the lump into guest memory can be accessed
    /// using [Self::get_len].
    fn get_data(&self, memory: GuestMemory<'_>, handle: u32, data_ptr: u32) -> Result<()> {
        let lump = self.get_lump(handle)?;
        let data_len = lump.bytes.len() as u32;
        let dst = memory.get_slice(data_ptr, data_len)?;
        dst.copy_from_slice(&lump.bytes);
        Ok(())
    }

    /// Unloads a lump by handle.
    fn free(&mut self, handle: u32) -> Result<()> {
        self.lump_handles
            .try_remove(handle as usize)
            .map(|_| ())
            .ok_or_else(|| anyhow!("lump handle {} is invalid", handle))
    }
}

impl LumpAbi {
    pub fn new(runtime: &Runtime, this_lump: LumpId) -> Self {
        Self {
            lump_store: runtime.lump_store.clone(),
            lump_handles: Default::default(),
            this_lump,
        }
    }

    /// Helper function to get a lump reference from a handle.
    fn get_lump(&self, handle: u32) -> Result<&LocalLump> {
        self.lump_handles
            .get(handle as usize)
            .ok_or_else(|| anyhow!("lump handle {} is invalid", handle))
    }
}

/// Implements the `hearth::table` ABI module.
pub struct TableAbi {
    process: Arc<Process>,
}

impl AsRef<Table> for TableAbi {
    fn as_ref(&self) -> &Table {
        self.process.borrow_table()
    }
}

#[impl_wasm_linker(module = "hearth::table")]
impl TableAbi {
    /// Increments the reference count of this capability.
    ///
    /// Guest-side capabilities will typically share handles, and making copies
    /// of them should use this function to communicate to the host that the
    /// guest is using that capability. If the guest plays along, this means
    /// that the host can reuse capability handles for identical capabilities,
    /// and inform the guest that incoming capabilities from messages or down
    /// signals are identical because of their shared handle.
    fn inc_ref(&self, handle: u32) -> Result<()> {
        self.as_ref()
            .inc_ref(CapabilityHandle(handle as usize))
            .with_context(|| format!("inc_ref({handle})"))?;

        Ok(())
    }

    /// Decrements the reference count of this capability.
    ///
    /// If the reference count of this handle drops to 0, it will be removed
    /// from the table.
    fn dec_ref(&self, handle: u32) -> Result<()> {
        self.as_ref()
            .dec_ref(CapabilityHandle(handle as usize))
            .with_context(|| format!("dec_ref({handle})"))?;

        Ok(())
    }

    /// Gets the permission flags of a capability.
    fn get_permissions(&self, handle: u32) -> Result<u32> {
        let perms = self
            .as_ref()
            .get_permissions(CapabilityHandle(handle as usize))
            .with_context(|| format!("get_permissions({handle})"))?;

        Ok(perms.bits())
    }

    /// Create a new capability from an existing one with a subset of the
    /// original's permissions.
    ///
    /// Fails if the desired permissions are not a subset of the original's.
    fn demote(&self, handle: u32, perms: u32) -> Result<u32> {
        let perms = Permissions::from_bits(perms).context("unknown permission bits set")?;

        let handle = self
            .as_ref()
            .demote(CapabilityHandle(handle as usize), perms)
            .with_context(|| format!("demote({handle})"))?;

        Ok(handle.0.try_into().unwrap())
    }

    /// Sends a message to a capability's route.
    ///
    /// `data_ptr` and `data_len` comprise a byte vector that is sent in the
    /// data payload of the message. `caps_ptr` and `caps_len` point to an
    /// array of `u32`-sized capability handles to be sent in the message.
    ///
    /// Fails if the capability does not have the send permission.
    async fn send(
        &self,
        memory: GuestMemory<'_>,
        handle: u32,
        data_ptr: u32,
        data_len: u32,
        caps_ptr: u32,
        caps_len: u32,
    ) -> Result<()> {
        let data = memory.get_slice(data_ptr, data_len)?;
        let caps = memory.get_memory_slice::<u32>(caps_ptr, caps_len)?;
        let caps: Vec<_> = caps
            .iter()
            .map(|cap| CapabilityHandle(*cap as usize))
            .collect();
        self.process
            .borrow_table()
            .send(CapabilityHandle(handle as usize), data, &caps)
            .await
            .with_context(|| format!("send({handle})"))?;

        Ok(())
    }

    /// Kills a capability's route group.
    ///
    /// Fails if the capability does not have the kill permission.
    fn kill(&self, handle: u32) -> Result<()> {
        self.as_ref()
            .kill(CapabilityHandle(handle as usize))
            .with_context(|| format!("kill({handle})"))?;

        Ok(())
    }
}

/// A form of signal mapped to a process's table.
enum Signal {
    Down { handle: u32 },
    Message { data: Vec<u8>, caps: Vec<u32> },
}

impl<'a> From<TableSignal<'a>> for Signal {
    fn from(signal: TableSignal<'a>) -> Signal {
        match signal {
            TableSignal::Down { handle } => Signal::Down {
                // TODO impl into for handles?
                handle: handle.0 as u32,
            },
            TableSignal::Message { data, caps } => Signal::Message {
                data: data.to_vec(),
                caps: caps.iter().map(|cap| cap.0 as u32).collect(),
            },
        }
    }
}

/// A data structure to contain a dynamically-allocated slab of mailboxes.
struct MailboxArena<'a> {
    group: &'a MailboxGroup<'a>,
    mbs: Slab<Mailbox<'a>>,
}

impl<'a> MailboxArena<'a> {
    /// Creates a new mailbox handle.
    ///
    /// Fails if the process has been killed.
    fn create(&mut self) -> Result<u32> {
        let mb = self
            .group
            .create_mailbox()
            .context("process has been killed")?;

        let handle = self.mbs.insert(mb);
        Ok(handle.try_into().unwrap())
    }
}

/// Implements the `hearth::mailbox` ABI module.
#[ouroboros::self_referencing]
pub struct MailboxAbi {
    process: Arc<Process>,
    signals: Slab<Signal>,

    #[borrows(process)]
    #[covariant]
    arena: MailboxArena<'this>,
}

#[impl_wasm_linker(module = "hearth::mailbox")]
impl MailboxAbi {
    /// Creates a new mailbox and returns its handle.
    fn create(&mut self) -> Result<u32> {
        Ok(self.with_arena_mut(|arena| arena.create())? + 1)
    }

    /// Destroys a mailbox by handle.
    fn destroy(&mut self, handle: u32) -> Result<()> {
        if handle == 0 {
            bail!("attempted to destroy parent mailbox");
        }

        self.with_arena_mut(|arena| {
            arena
                .mbs
                .try_remove(handle as usize - 1)
                .context("invalid handle")
        })?;

        Ok(())
    }

    /// Make a capability in this process's table to a mailbox with the given
    /// permissions.
    fn make_capability(&self, handle: u32, perms: u32) -> Result<u32> {
        let mb = self.get_mb(handle)?;
        let perms = Permissions::from_bits(perms).context("unknown permission bits set")?;
        let cap = mb
            .export(perms, self.borrow_process().borrow_table())
            .unwrap();
        Ok(cap.into_handle().0.try_into().unwrap())
    }

    /// Monitors a capability by its handle in this process's table. When the
    /// capability is closed, the mailbox will receive a down signal.
    fn monitor(&self, mailbox: u32, cap: u32) -> Result<()> {
        let cap = CapabilityHandle(cap as usize);
        let mb = self.get_mb(mailbox)?;

        self.borrow_process()
            .borrow_table()
            .monitor(cap, mb)
            .with_context(|| format!("monitor(mailbox = {}, cap = {})", mailbox, cap.0))?;

        Ok(())
    }

    /// Waits for a signal to be received by a mailbox.
    async fn recv(&mut self, handle: u32) -> Result<u32> {
        let mb = self.get_mb(handle)?;

        let signal = mb
            .recv(|signal| Signal::from(signal))
            .await
            .context("process has been killed")?;

        let handle = self.with_signals_mut(|signals| signals.insert(signal));

        Ok(handle.try_into().unwrap())
    }

    /// Checks if a mailbox has received any signals without waiting.
    ///
    /// Returns `u32::MAX` (or `0xFFFFFFFF`) if the mailbox's queue is empty.
    /// Otherwise, returns the handle to the received signal.
    fn try_recv(&mut self, handle: u32) -> Result<u32> {
        let mb = self.get_mb(handle)?;

        let signal = mb
            .try_recv(|signal| Signal::from(signal))
            .context("process has been killed")?;

        match signal {
            Some(signal) => {
                let handle = self.with_signals_mut(|signals| signals.insert(signal));
                Ok(handle.try_into().unwrap())
            }
            None => Ok(u32::MAX),
        }
    }

    /// Waits for one of multiple mailboxes to receive a signal.
    ///
    /// `handles_ptr` and `handles_len` point to an array of `u32`-sized
    /// mailbox handles in guest memory.
    ///
    /// Returns two 32-bit values encoded in a 64-bit integer. The upper 32
    /// bits of the return value encode the index of the mailbox in the handles
    /// array that received the signal. The lower 32 bits encode the handle of
    /// the received signal itself.
    async fn poll(
        &mut self,
        memory: GuestMemory<'_>,
        handles_ptr: u32,
        handles_len: u32,
    ) -> Result<u64> {
        let handles = memory.get_memory_slice(handles_ptr, handles_len)?;

        let mbs = handles
            .iter()
            .map(|handle| self.get_mb(*handle))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|mb| mb.recv(|signal| Signal::from(signal)))
            .map(Box::pin);

        let (signal, index, _) = futures_util::future::select_all(mbs).await;
        let signal = signal.context("process has been killed")?;
        let handle = self.with_signals_mut(|signals| signals.insert(signal));
        let result = ((index as u64) << 32) | (handle as u64);
        Ok(result)
    }

    /// Frees a signal by handle.
    fn destroy_signal(&mut self, handle: u32) -> Result<()> {
        self.with_signals_mut(|signals| signals.try_remove(handle as usize))
            .map(|_| ())
            .context("invalid handle")
    }

    /// Gets the kind of a signal by its handle.
    ///
    /// See [SignalKind] for the possible values.
    fn get_signal_kind(&self, handle: u32) -> Result<u32> {
        let signal = self.get_signal(handle)?;

        let kind = match signal {
            Signal::Down { .. } => SignalKind::Down,
            Signal::Message { .. } => SignalKind::Message,
        };

        Ok(kind.into())
    }

    /// Gets the inner capability handle of a down signal.
    ///
    /// Fails if the given signal is not a down signal.
    fn get_down_capability(&self, handle: u32) -> Result<u32> {
        let signal = self.get_signal(handle)?;

        let Signal::Down { handle } = signal else {
            bail!("invalid signal kind");
        };

        Ok(*handle)
    }

    /// Gets the length of the data in a message signal.
    ///
    /// Fails if the given signal is not a message signal.
    fn get_message_data_len(&self, handle: u32) -> Result<u32> {
        let (data, _caps) = self.get_message(handle)?;
        Ok(data.len().try_into().unwrap())
    }

    /// Gets the data in a message signal.
    ///
    /// The required size of the data can be retrieved with
    /// [Self::get_message_data_len].
    ///
    /// Fails if the given signal is not a message signal.
    fn get_message_data(&self, memory: GuestMemory<'_>, handle: u32, dst_ptr: u32) -> Result<()> {
        let (data, _caps) = self.get_message(handle)?;
        let dst_len = data.len().try_into().unwrap();
        let dst = memory.get_memory_slice(dst_ptr, dst_len)?;
        dst.copy_from_slice(data);
        Ok(())
    }

    /// Gets the length of the capability list in a message signal.
    ///
    /// Fails if the given signal is not a message signal.
    fn get_message_caps_num(&self, handle: u32) -> Result<u32> {
        let (_data, caps) = self.get_message(handle)?;
        Ok(caps.len().try_into().unwrap())
    }

    /// Gets the capability list in a message signal.
    ///
    /// The required number of handles can be retrieved with
    /// [Self::get_message_caps_len].
    ///
    /// Fails if the given signal is not a message signal.
    fn get_message_caps(&self, memory: GuestMemory<'_>, handle: u32, dst_ptr: u32) -> Result<()> {
        let (_data, caps) = self.get_message(handle)?;
        let dst_len = caps.len().try_into().unwrap();
        let dst = memory.get_memory_slice(dst_ptr, dst_len)?;
        dst.copy_from_slice(caps);
        Ok(())
    }
}

impl MailboxAbi {
    /// Helper function to get a reference to a mailbox by its handle.
    ///
    /// Fails if the handle is invalid.
    fn get_mb(&self, handle: u32) -> Result<&Mailbox> {
        if handle == 0 {
            Ok(self.borrow_process().borrow_parent())
        } else {
            self.with_arena(|arena| arena.mbs.get(handle as usize - 1))
                .context("invalid handle")
        }
    }

    /// Helper function to get a reference to a signal by its handle.
    ///
    /// Fails if the handle is invalid.
    fn get_signal(&self, handle: u32) -> Result<&Signal> {
        self.with_signals(|signals| signals.get(handle as usize))
            .context("invalid handle")
    }

    /// Helper function to get a message signal by its handle.
    ///
    /// Fails if the handle is invalid or if the signal is not a message.
    fn get_message(&self, handle: u32) -> Result<(&[u8], &[u32])> {
        let signal = self.get_signal(handle)?;

        let Signal::Message { data, caps } = signal else {
            bail!("invalid signal kind");
        };

        Ok((data, caps))
    }
}

/// Encapsulates an instance of each guest ABI data structure.
pub struct ProcessData {
    pub log: LogAbi,
    pub lump: LumpAbi,
    pub table: TableAbi,
    pub mailbox: MailboxAbi,
}

impl ProcessData {
    pub fn new(runtime: &Runtime, process: Process, this_lump: LumpId) -> Self {
        let process = Arc::new(process);

        Self {
            log: LogAbi {
                process: process.clone(),
            },
            lump: LumpAbi::new(runtime, this_lump),
            table: TableAbi {
                process: process.clone(),
            },
            mailbox: MailboxAbi::new(process, Slab::new(), |process| MailboxArena {
                group: process.borrow_group(),
                mbs: Slab::new(),
            }),
        }
    }
}

macro_rules! impl_asmut {
    ($ty: ident, $sub_ty: ident, $sub_field: ident) => {
        impl ::std::convert::AsMut<$sub_ty> for $ty {
            fn as_mut(&mut self) -> &mut $sub_ty {
                &mut self.$sub_field
            }
        }
    };
}

impl_asmut!(ProcessData, LogAbi, log);
impl_asmut!(ProcessData, LumpAbi, lump);
impl_asmut!(ProcessData, TableAbi, table);
impl_asmut!(ProcessData, MailboxAbi, mailbox);

impl ProcessData {
    /// Adds all module ABIs to the given linker.
    pub fn add_to_linker(linker: &mut Linker<Self>) {
        LogAbi::add_to_linker(linker);
        LumpAbi::add_to_linker(linker);
        TableAbi::add_to_linker(linker);
        MailboxAbi::add_to_linker(linker);
    }
}

struct WasmProcess {
    engine: Arc<Engine>,
    linker: Arc<Linker<ProcessData>>,
    module: Arc<Module>,
    this_lump: LumpId,
    entrypoint: Option<u32>,
}

impl WasmProcess {
    /// Executes a Wasm process.
    async fn run(mut self, runtime: Arc<Runtime>, ctx: Process) {
        let pid = ctx.borrow_info().pid;

        match self
            .run_inner(runtime, ctx)
            .await
            .with_context(|| format!("PID {}", pid))
        {
            Ok(()) => {}
            Err(err) => {
                error!("{:?}", err);
            }
        }
    }

    /// Performs the actual process execution using easy error handling.
    async fn run_inner(&mut self, runtime: Arc<Runtime>, ctx: Process) -> Result<()> {
        // TODO log using the process log instead of tracing?
        let data = ProcessData::new(runtime.as_ref(), ctx, self.this_lump);
        let mut store = Store::new(&self.engine, data);

        store.epoch_deadline_callback(move |store| {
            if store.data().table.process.borrow_group().poll_dead() {
                bail!("process killed");
            }

            Ok(UpdateDeadline::Yield(1))
        });

        let instance = self
            .linker
            .instantiate_async(&mut store, &self.module)
            .await
            .context("instantiating Wasm instance")?;

        let init = instance.get_typed_func::<(), ()>(&mut store, "_hearth_init");
        if let Ok(init) = init {
            init.call_async(&mut store, ())
                .await
                .context("calling Wasm init function")?;
        }

        if let Some(entrypoint) = self.entrypoint {
            let cb = instance
                .get_typed_func::<u32, ()>(&mut store, "_hearth_spawn_by_index")
                .context("lookup _hearth_spawn_by_index")?;
            cb.call_async(&mut store, entrypoint)
                .await
                .context("calling Wasm entrypoint")?;
        } else {
            let cb = instance.get_typed_func::<(), ()>(&mut store, "run")?;
            cb.call_async(&mut store, ())
                .await
                .context("calling Wasm run()")?;
        }

        Ok(())
    }
}

pub struct WasmProcessSpawner {
    engine: Arc<Engine>,
    linker: Arc<Linker<ProcessData>>,
}

#[async_trait]
impl RequestResponseProcess for WasmProcessSpawner {
    type Request = WasmSpawnInfo;
    type Response = ();

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, WasmSpawnInfo>,
    ) -> ResponseInfo<'a, Self::Response> {
        let module = request
            .runtime
            .asset_store
            .load_asset::<WasmModuleLoader>(&request.data.lump)
            .await
            .context("loading Wasm module");

        let module = match module {
            Ok(module) => module,
            Err(err) => {
                error!("failed to load Wasm module: {:?}", err);
                return ().into();
            }
        };

        debug!("Spawning module {}", request.data.lump);
        let meta = ProcessMetadata::default();
        let child = request.runtime.process_factory.spawn(meta);

        // create a capability to this child's parent mailbox
        let perms = Permissions::SEND | Permissions::MONITOR | Permissions::KILL;
        // TODO https://github.com/hearth-rs/flue/pull/9 makes this cleaner
        let child_cap = child.borrow_parent().export_owned(perms);
        let child_cap = request
            .process
            .borrow_table()
            .import_owned(child_cap)
            .unwrap();
        let child_cap = request
            .process
            .borrow_table()
            .wrap_handle(child_cap)
            .unwrap();

        // send initial capabilities
        child_cap
            .send(&[], request.cap_args.iter().collect::<Vec<_>>().as_slice())
            .await
            .unwrap();

        // flush initial capabilities
        child.borrow_parent().recv(|_| ()).await.unwrap();

        let process = WasmProcess {
            engine: self.engine.clone(),
            linker: self.linker.clone(),
            module,
            entrypoint: request.data.entrypoint,
            this_lump: request.data.lump,
        };

        let runtime = request.runtime.clone();
        tokio::spawn(process.run(runtime, child));

        ResponseInfo {
            data: (),
            caps: vec![child_cap],
        }
    }
}

impl ServiceRunner for WasmProcessSpawner {
    const NAME: &'static str = "hearth.cognito.WasmProcessSpawner";

    fn get_process_metadata() -> ProcessMetadata {
        let mut meta = cargo_process_metadata!();
        meta.description =
            Some("The native WebAssembly process spawner. Accepts WasmSpawnInfo.".to_string());

        meta
    }
}

pub struct WasmModuleLoader {
    engine: Arc<Engine>,
}

#[async_trait]
impl AssetLoader for WasmModuleLoader {
    type Asset = Module;

    async fn load_asset(&self, data: &[u8]) -> anyhow::Result<Module> {
        Module::new(&self.engine, data)
    }
}

pub struct WasmPlugin {
    engine: Arc<Engine>,
}

impl Default for WasmPlugin {
    fn default() -> Self {
        let mut config = Config::new();
        config.async_support(true);
        config.epoch_interruption(true);
        config.memory_init_cow(true);

        let engine = Engine::new(&config).unwrap();

        Self {
            engine: Arc::new(engine),
        }
    }
}

impl Plugin for WasmPlugin {
    fn build(&mut self, builder: &mut RuntimeBuilder) {
        let mut linker = Linker::new(&self.engine);
        ProcessData::add_to_linker(&mut linker);

        builder.add_plugin(WasmProcessSpawner {
            engine: self.engine.to_owned(),
            linker: Arc::new(linker),
        });

        builder.add_asset_loader(WasmModuleLoader {
            engine: self.engine.to_owned(),
        });
    }

    fn finalize(self, _builder: &mut RuntimeBuilder) {
        tokio::spawn(async move {
            // TODO make this time slice duration configurable
            let duration = std::time::Duration::from_micros(100);
            loop {
                tokio::time::sleep(duration).await;
                self.engine.increment_epoch();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link() {
        let mut config = Config::new();
        config.async_support(true);
        let engine = Engine::new(&config).unwrap();
        let mut linker = Linker::new(&engine);
        ProcessData::add_to_linker(&mut linker);
    }
}
