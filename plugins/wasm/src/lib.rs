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

use hearth_runtime::anyhow::{anyhow, bail, Context, Result};
use hearth_runtime::asset::{AssetLoader, AssetStore};
use hearth_runtime::flue::{
    CapabilityHandle, CapabilityRef, Mailbox, MailboxGroup, Permissions, Table, TableSignal,
};
use hearth_runtime::hearth_macros::{impl_wasm_linker, GetProcessMetadata};
use hearth_runtime::lump::{bytes::Bytes, LumpStoreImpl};
use hearth_runtime::process::{Process, ProcessMetadata};
use hearth_runtime::runtime::{Plugin, Runtime, RuntimeBuilder};
use hearth_runtime::{async_trait, hearth_schema};
use hearth_runtime::{tokio, utils::*};
use hearth_schema::wasm::WasmSpawnInfo;
use hearth_schema::{LumpId, ProcessLogLevel, SignalKind};
use slab::Slab;
use tracing::{error, warn};
use wasmtime::{Caller, Config, Engine, Instance, Linker, Module, Store, UpdateDeadline};

/// An interface to attempt to acquire a Wasm ABI by type.
pub trait GetAbi<T>
where
    T: Sized,
{
    /// Attempt to get a mutable reference to an ABI in this process data.
    fn get_abi(&mut self) -> Result<&mut T>;
}

/// An interface for Wasm ABIs: host-side data exposed to WebAssembly through a
/// set of linked host functions.
///
/// Implemented by the [impl_wasm_linker] proc macro.
pub trait WasmLinker<T: GetAbi<Self>>: Sized {
    /// Add this ABI's functions to the given Linker.
    fn add_to_linker(linker: &mut Linker<T>);
}

/// A utility type for safely accessing and interpreting a Wasm guest's memory.
pub struct GuestMemory<'a> {
    pub bytes: &'a mut [u8],
}

impl<'a> GuestMemory<'a> {
    /// Access a Wasm host function's caller's memory.
    ///
    /// Fails if the caller does not export its memory correctly.
    pub fn from_caller<T>(caller: &mut Caller<'a, T>) -> Result<Self> {
        let memory = caller
            .get_export("memory")
            .ok_or_else(|| anyhow!("Caller does not export memory"))?
            .into_memory()
            .ok_or_else(|| anyhow!("Caller 'memory' export is not a memory"))?;
        let data_ptr = memory.data_ptr(&caller);
        let data_size = memory.data_size(&caller);
        let bytes = unsafe { std::slice::from_raw_parts_mut(data_ptr, data_size) };
        Ok(Self { bytes })
    }

    /// Interprets a region of guest memory as a string.
    ///
    /// Fails if out-of-bounds.
    pub fn get_str(&self, ptr: u32, len: u32) -> Result<&'a mut str> {
        let memory = self.get_slice(ptr, len)?;
        std::str::from_utf8_mut(memory)
            .with_context(|| format!("GuestMemory::get_str({}, {})", ptr, len))
    }

    /// Retrieves a byte slice of guest memory by its pointer and length.
    ///
    /// Fails if out-of-bounds.
    pub fn get_slice(&self, ptr: u32, len: u32) -> Result<&'a mut [u8]> {
        let ptr = ptr as usize;
        let len = len as usize;
        if ptr + len > self.bytes.len() {
            Err(anyhow!(
                "GuestMemory::get_slice({}, {}) is out-of-bounds",
                ptr,
                len
            ))
        } else {
            unsafe {
                let ptr = self.bytes.as_ptr().add(ptr) as *mut u8;
                Ok(std::slice::from_raw_parts_mut(ptr, len))
            }
        }
    }

    /// Interprets a region of guest memory as a data structure.
    ///
    /// Fails if out-of-bounds.
    pub fn get_memory_ref<T: bytemuck::Pod>(&self, ptr: u32) -> Result<&'a mut T> {
        let len = std::mem::size_of::<T>() as u32;
        let bytes = self.get_slice(ptr, len)?;
        bytemuck::try_from_bytes_mut(bytes).map_err(|err| {
            anyhow!(
                "GuestMemory::get_memory_ref<{}>({}) failed: {:?}",
                std::any::type_name::<T>(),
                ptr,
                err
            )
        })
    }

    /// Interprets a region of guest memory as an array of a data structure.
    ///
    /// Fails if out-of-bounds.
    pub fn get_memory_slice<T: bytemuck::Pod>(&self, ptr: u32, num: u32) -> Result<&'a mut [T]> {
        let len = num * std::mem::size_of::<T>() as u32;
        let bytes = self.get_slice(ptr, len)?;
        bytemuck::try_cast_slice_mut(bytes).map_err(|err| {
            anyhow!(
                "GuestMemory::get_memory_slice<{}>({}, {}) failed: {:?}",
                std::any::type_name::<T>(),
                ptr,
                num,
                err
            )
        })
    }
}

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

        let module = memory.get_str(module_ptr, module_len)?.to_string();
        let content = memory.get_str(content_ptr, content_len)?.to_string();

        let info = self.process.borrow_info();
        info.process_span.in_scope(|| match level {
            ProcessLogLevel::Trace => tracing::trace!(module, "{content}"),

            ProcessLogLevel::Debug => tracing::debug!(module, "{content}"),
            ProcessLogLevel::Info => tracing::info!(module, "{content}"),
            ProcessLogLevel::Warning => tracing::warn!(module, "{content}"),
            ProcessLogLevel::Error => tracing::error!(module, "{content}"),
        });

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
        let cap = mb.export(perms).unwrap();
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

/// Implements the `hearth::metadata` ABI module.
///
/// This ABI is only available during the metadata stage of process execution.
/// Its role is to write each field of [ProcessMetadata] before the process is
/// actually spawned with access to the full runtime. Each method modifies a
/// given field of [ProcessMetadata].
#[derive(Default)]
pub struct MetadataAbi {
    meta: ProcessMetadata,
}

#[impl_wasm_linker(module = "hearth::metadata")]
impl MetadataAbi {
    fn set_name(&mut self, memory: GuestMemory<'_>, ptr: u32, len: u32) -> Result<()> {
        let str = memory.get_str(ptr, len)?;
        self.meta.name = Some(str.to_string());
        Ok(())
    }

    fn set_description(&mut self, memory: GuestMemory<'_>, ptr: u32, len: u32) -> Result<()> {
        let str = memory.get_str(ptr, len)?;
        self.meta.description = Some(str.to_string());
        Ok(())
    }

    fn add_author(&mut self, memory: GuestMemory<'_>, ptr: u32, len: u32) -> Result<()> {
        let str = memory.get_str(ptr, len)?;

        self.meta
            .authors
            .get_or_insert(Default::default())
            .push(str.to_string());

        Ok(())
    }

    fn set_repository(&mut self, memory: GuestMemory<'_>, ptr: u32, len: u32) -> Result<()> {
        let str = memory.get_str(ptr, len)?;
        self.meta.repository = Some(str.to_string());
        Ok(())
    }

    fn set_homepage(&mut self, memory: GuestMemory<'_>, ptr: u32, len: u32) -> Result<()> {
        let str = memory.get_str(ptr, len)?;
        self.meta.homepage = Some(str.to_string());
        Ok(())
    }

    fn set_license(&mut self, memory: GuestMemory<'_>, ptr: u32, len: u32) -> Result<()> {
        let str = memory.get_str(ptr, len)?;
        self.meta.license = Some(str.to_string());
        Ok(())
    }
}

/// Encapsulates an instance of each guest ABI data structure.
///
/// Each variant is only accessible during a specific phase of a process's
/// execution. If a process attempts to access an ABI that isn't available in
/// its phase, the [GetAbi] implementation for that ABI will throw an error.
pub enum ProcessData {
    /// The **metadata phase** of process execution.
    ///
    /// Before the process is spawned into the runtime, it may export
    /// user-facing metadata through [MetadataAbi].
    Metadata { metadata: MetadataAbi },

    /// The **running phase** of process execution.
    ///
    /// Provides full access to a process's ABIs post-spawn.
    Running {
        log: LogAbi,
        lump: LumpAbi,
        table: TableAbi,
        mailbox: MailboxAbi,
    },
}

impl GetAbi<MetadataAbi> for ProcessData {
    fn get_abi(&mut self) -> Result<&mut MetadataAbi> {
        match self {
            Self::Running { .. } => bail!("process is running"),
            Self::Metadata { metadata } => Ok(metadata),
        }
    }
}

macro_rules! impl_running_get_abi {
    ($ty: ident, $sub_ty: ident, $sub_field: ident) => {
        impl GetAbi<$sub_ty> for $ty {
            fn get_abi(&mut self) -> Result<&mut $sub_ty> {
                match self {
                    Self::Metadata { .. } => bail!("process is not running"),
                    Self::Running { $sub_field, .. } => Ok($sub_field),
                }
            }
        }
    };
}

impl_running_get_abi!(ProcessData, LogAbi, log);
impl_running_get_abi!(ProcessData, LumpAbi, lump);
impl_running_get_abi!(ProcessData, TableAbi, table);
impl_running_get_abi!(ProcessData, MailboxAbi, mailbox);

impl ProcessData {
    pub fn new_metadata() -> Self {
        Self::Metadata {
            metadata: Default::default(),
        }
    }

    pub fn new_running(runtime: &Runtime, process: Process, this_lump: LumpId) -> Self {
        let process = Arc::new(process);

        Self::Running {
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

    /// Adds all module ABIs to the given linker.
    pub fn add_to_linker(linker: &mut Linker<Self>) {
        LogAbi::add_to_linker(linker);
        LumpAbi::add_to_linker(linker);
        TableAbi::add_to_linker(linker);
        MailboxAbi::add_to_linker(linker);
        MetadataAbi::add_to_linker(linker);
    }
}

struct WasmProcess {
    store: Store<ProcessData>,
    exports_metadata: bool,
    instance: Instance,
    this_lump: LumpId,
}

impl WasmProcess {
    pub async fn new(
        engine: &Engine,
        linker: &Linker<ProcessData>,
        module: &Module,
        this_lump: LumpId,
    ) -> Result<Self> {
        let data = ProcessData::new_metadata();
        let mut store = Store::new(engine, data);

        let instance = linker
            .instantiate_async(&mut store, module)
            .await
            .context("instantiating Wasm instance")?;

        Ok(Self {
            store,
            exports_metadata: false,
            instance,
            this_lump,
        })
    }

    /// Executes the process's `_hearth_metadata` function and returns the
    /// result.
    pub async fn get_metadata(&mut self) -> Result<ProcessMetadata> {
        // while retrieving the process metadata, preemptively timeslice
        self.store.epoch_deadline_async_yield_and_update(1);

        // attempt to locate the `_hearth_metadata` export
        if let Ok(cb) = self
            .instance
            .get_typed_func(&mut self.store, "_hearth_metadata")
        {
            // attempt to call it
            cb.call_async(&mut self.store, ())
                .await
                .context("calling Wasm metadata function")?;

            // signal that the metadata was exported
            self.exports_metadata = true;
        }

        // retrieve the written metadata from the store's process data
        let ProcessData::Metadata { metadata } = self.store.data() else {
            bail!("process metadata unavailable");
        };

        Ok(metadata.meta.to_owned())
    }

    /// Executes a Wasm process.
    async fn run(mut self, runtime: Arc<Runtime>, ctx: Process, entrypoint: Option<u32>) {
        // grab the PID for logging
        let pid = ctx.borrow_info().pid;

        // log a warning if this process did not export its metadata
        if !self.exports_metadata {
            warn!(
                "Wasm guest with PID {} did not export its process metadata",
                pid
            );
        }

        // switch the process ABIs to running
        *self.store.data_mut() = ProcessData::new_running(runtime.as_ref(), ctx, self.this_lump);

        // while executing the main function, preemptively timeslice until killed
        self.store.epoch_deadline_callback(move |store| {
            let ProcessData::Running { table, .. } = store.data() else {
                bail!("process is not running");
            };

            if table.process.borrow_group().poll_dead() {
                bail!("process killed");
            }

            Ok(UpdateDeadline::Yield(1))
        });

        // call inner execution behavior and handle its errors
        match self
            .run_inner(entrypoint)
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
    async fn run_inner(&mut self, entrypoint: Option<u32>) -> Result<()> {
        // run the `_hearth_init` export, if available
        if let Ok(init) = self
            .instance
            .get_typed_func(&mut self.store, "_hearth_init")
        {
            init.call_async(&mut self.store, ())
                .await
                .context("calling Wasm init function")?;
        }

        // switch on which entrypoint function to call
        match entrypoint {
            // no entrypoint index given
            None => {
                // retrieve the `run` export
                let cb = self
                    .instance
                    .get_typed_func(&mut self.store, "run")
                    .context("lookup run")?;

                // execute it
                cb.call_async(&mut self.store, ())
                    .await
                    .context("calling Wasm run()")
            }
            // execute a specific entrypoint by index
            Some(entrypoint) => {
                // retrieve the `_hearth_spawn_by_index` export
                let cb = self
                    .instance
                    .get_typed_func(&mut self.store, "_hearth_spawn_by_index")
                    .context("lookup _hearth_spawn_by_index")?;

                // call it with the specified entrypoint index
                cb.call_async(&mut self.store, entrypoint)
                    .await
                    .context("calling Wasm entrypoint")
            }
        }
    }
}

/// The native WebAssembly process spawner. Accepts WasmSpawnInfo.
#[derive(GetProcessMetadata)]
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
        ResponseInfo {
            data: (),
            caps: match self.spawn(request).await {
                // spawned successfully; return cap
                Ok(child) => vec![child],
                // error occurred. log and no cap
                Err(err) => {
                    error!("Wasm spawning error: {:?}", err);
                    vec![]
                }
            },
        }
    }
}

impl ServiceRunner for WasmProcessSpawner {
    const NAME: &'static str = "hearth.wasm.WasmProcessSpawner";
}

impl WasmProcessSpawner {
    pub async fn spawn<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, WasmSpawnInfo>,
    ) -> Result<CapabilityRef<'a>> {
        // load the WebAssembly module from the asset store
        let module = request
            .runtime
            .asset_store
            .load_asset::<WasmModuleLoader>(&request.data.lump)
            .await
            .context("loading Wasm module")?;

        // instantiate a new WasmProcess
        let mut process = WasmProcess::new(&self.engine, &self.linker, &module, request.data.lump)
            .await
            .context("initializing process")?;

        // retrieve the process's metadata
        let meta = process
            .get_metadata()
            .await
            .context("retrieving process metadata")?;

        // spawn a new local process
        let child = request.runtime.process_factory.spawn(meta);

        // import a capability to its parent mailbox
        let child_cap = child
            .borrow_parent()
            .export_to(Permissions::all(), request.process.borrow_table())
            .unwrap();

        // send the child the initial capabilities from the request
        child_cap
            .send(&[], request.cap_args.iter().collect::<Vec<_>>().as_slice())
            .await
            .unwrap();

        // flush the child's mailbox to import the initial capabilities
        child.borrow_parent().recv(|_| ()).await.unwrap();

        // run the process
        let runtime = request.runtime.clone();
        tokio::spawn(process.run(runtime, child, request.data.entrypoint));

        // return the child's cap
        Ok(child_cap)
    }
}

pub struct WasmModuleLoader {
    engine: Arc<Engine>,
}

#[async_trait]
impl AssetLoader for WasmModuleLoader {
    type Asset = Module;

    async fn load_asset(&self, _store: &AssetStore, data: &[u8]) -> Result<Module> {
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
