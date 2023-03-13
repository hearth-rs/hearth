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

pub use hearth_types::*;

/// Internal helper function to turn a string into a pointer and length.
fn abi_string(str: &str) -> (u32, u32) {
    let bytes = str.as_bytes();
    let ptr = bytes.as_ptr() as u32;
    let len = bytes.len() as u32;
    (ptr, len)
}

/// Fetches the process ID of the current process.
pub fn this_pid() -> ProcessId {
    let pid = unsafe { abi::process::this_pid() };
    ProcessId(pid)
}

/// Looks up the process ID of a peer's service by name.
pub fn service_lookup(peer: PeerId, name: &str) -> Option<ProcessId> {
    let (name_ptr, name_len) = abi_string(name);
    let pid = unsafe { abi::service::lookup(peer.0, name_ptr, name_len) };
    if pid == u64::MAX {
        None
    } else {
        Some(ProcessId(pid))
    }
}

/// Registers a process as a service on its peer.
pub fn service_register(pid: ProcessId, name: &str) {
    let (name_ptr, name_len) = abi_string(name);
    unsafe { abi::service::register(pid.0, name_ptr, name_len) }
}

/// Deregisters a peer's service.
pub fn service_deregister(peer: PeerId, name: &str) {
    let (name_ptr, name_len) = abi_string(name);
    unsafe { abi::service::deregister(peer.0, name_ptr, name_len) }
}

/// Kills a process.
pub fn kill(pid: ProcessId) {
    unsafe { abi::process::kill(pid.0) }
}

/// Sends a message to another process.
pub fn send(pid: ProcessId, data: &[u8]) {
    unsafe { abi::message::send(pid.0, data.as_ptr() as u32, data.len() as u32) }
}

/// Blocks until a message has been received.
pub fn recv() -> Message {
    let msg = unsafe { abi::message::recv() };
    Message(msg)
}

/// Blocks until a message is received or the given timeout (in microseconds)
/// expires.
///
/// Setting the timeout to 0 skips any blocking and in effect polls the message
/// queue for a new message.
pub fn recv_timeout(timeout_us: u64) -> Message {
    let msg = unsafe { abi::message::recv_timeout(timeout_us) };
    Message(msg)
}

/// A message that has been received from another process.
pub struct Message(u32);

impl Drop for Message {
    fn drop(&mut self) {
        unsafe { abi::message::free(self.0) }
    }
}

impl Message {
    /// Gets the ID of the process that sent this message.
    pub fn get_sender(&self) -> ProcessId {
        let pid = unsafe { abi::message::get_sender(self.0) };
        ProcessId(pid)
    }

    /// Reads out the message data into an owning byte vector.
    pub fn get_data(&self) -> Vec<u8> {
        #[allow(clippy::uninit_vec)]
        unsafe {
            let len = abi::message::get_len(self.0) as usize;
            let mut data = Vec::with_capacity(len);
            data.set_len(len);
            abi::message::get_data(self.0, data.as_ptr() as u32);
            data
        }
    }
}

/// A loaded lump.
pub struct Lump(u32);

impl Drop for Lump {
    fn drop(&mut self) {
        unsafe { abi::lump::free(self.0) }
    }
}

impl Lump {
    /// Loads a new lump from in-process data.
    pub fn load(data: &[u8]) -> Self {
        unsafe {
            let ptr = data.as_ptr() as u32;
            let len = data.len() as u32;
            let handle = abi::lump::load(ptr, len);
            Self(handle)
        }
    }

    /// Loads a lump from the ID of an already existing lump.
    pub fn from_id(id: &LumpId) -> Self {
        unsafe {
            let handle = abi::lump::from_id(id as *const LumpId as u32);
            Self(handle)
        }
    }

    /// Gets the ID of this lump.
    pub fn get_id(&self) -> LumpId {
        unsafe {
            let id = LumpId(Default::default());
            let id_ptr = &id as *const LumpId as u32;
            abi::lump::get_id(self.0, id_ptr);
            id
        }
    }

    /// Retrieves the data stored in this lump.
    pub fn get_data(&self) -> Vec<u8> {
        #[allow(clippy::uninit_vec)]
        unsafe {
            let len = abi::lump::get_len(self.0) as usize;
            let mut data = Vec::with_capacity(len);
            data.set_len(len);
            abi::lump::get_data(self.0, data.as_ptr() as u32);
            data
        }
    }
}

/// A loaded asset.
pub struct Asset(u32);

impl Drop for Asset {
    fn drop(&mut self) {
        unsafe { abi::asset::free(self.0) }
    }
}

impl Asset {
    /// Loads an asset from a lump.
    pub fn load(lump: &Lump, class: &str) -> Self {
        unsafe {
            let (class_ptr, class_len) = abi_string(class);
            let handle = abi::asset::load(lump.0, class_ptr, class_len);
            Self(handle)
        }
    }

    /// Returns the internal handle ID of this asset.
    pub fn get_id(&self) -> u32 {
        self.0
    }
}

/// Log a message.
pub fn log(level: ProcessLogLevel, target: &str, content: &str) {
    let level = level.into();
    let (target_ptr, target_len) = abi_string(target);
    let (content_ptr, content_len) = abi_string(content);
    unsafe { abi::log::log(level, target_ptr, target_len, content_ptr, content_len) }
}

#[allow(clashing_extern_declarations)]
mod abi {
    pub mod asset {
        #[link(wasm_import_module = "hearth::asset")]
        extern "C" {
            pub fn load(lump_handle: u32, class_ptr: u32, class_len: u32) -> u32;
            pub fn free(lump_handle: u32);
        }
    }

    pub mod log {
        #[link(wasm_import_module = "hearth::log")]
        extern "C" {
            pub fn log(
                level: u32,
                target_ptr: u32,
                target_len: u32,
                content_ptr: u32,
                content_len: u32,
            );
        }
    }

    pub mod lump {
        #[link(wasm_import_module = "hearth::lump")]
        extern "C" {
            pub fn from_id(id_ptr: u32) -> u32;
            pub fn load(ptr: u32, len: u32) -> u32;
            pub fn get_id(handle: u32, id_ptr: u32);
            pub fn get_len(handle: u32) -> u32;
            pub fn get_data(handle: u32, ptr: u32);
            pub fn free(handle: u32);
        }
    }

    pub mod message {
        #[link(wasm_import_module = "hearth::message")]
        extern "C" {
            pub fn recv() -> u32;
            pub fn recv_timeout(timeout_us: u64) -> u32;
            pub fn send(pid: u64, ptr: u32, len: u32);
            pub fn get_sender(msg: u32) -> u64;
            pub fn get_len(msg: u32) -> u32;
            pub fn get_data(msg: u32, ptr: u32);
            pub fn free(msg: u32);
        }
    }

    pub mod process {
        #[link(wasm_import_module = "hearth::process")]
        extern "C" {
            pub fn this_pid() -> u64;
            pub fn kill(pid: u64);
        }
    }

    pub mod service {
        #[link(wasm_import_module = "hearth::service")]
        extern "C" {
            pub fn lookup(peer: u32, name_ptr: u32, name_len: u32) -> u64;
            pub fn register(pid: u64, name_ptr: u32, name_len: u32);
            pub fn deregister(peer: u32, name_ptr: u32, name_len: u32);
        }
    }
}
