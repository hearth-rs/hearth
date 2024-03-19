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

//! Safe Rust bindings over the Hearth host API.

#![warn(missing_docs)]

mod subscriber;

use std::borrow::Borrow;

use serde::{Deserialize, Serialize};

pub use hearth_schema::*;
use subscriber::ProcessSubscriber;

/// Internal helper function to turn a string into a pointer and length.
fn abi_string(str: &str) -> (u32, u32) {
    let bytes = str.as_bytes();
    let ptr = bytes.as_ptr() as u32;
    let len = bytes.len() as u32;
    (ptr, len)
}

/// Fetches the lump ID of the module used to spawn the current process.
pub fn this_lump() -> LumpId {
    // load lump ID from the host
    let mut id = LumpId(Default::default());
    unsafe { abi::lump::this_lump(&mut id as *const LumpId as u32) }
    id
}

/// An integer handle to a capability to a route.
///
/// If two capabilities are to the same route and have the same permissions,
/// then testing their equality (`cap1 == cap2`) will evaluate to true.
/// However, if the permissions are different on either capability, they will
/// never be identical.
///
/// Capability handles are reference-counted, so you can clone and drop this
/// type to increase and decrease the reference count of this capability in the
/// underlying capability table host-side.
#[repr(transparent)]
#[derive(Debug, Hash, PartialEq, Eq)]
pub struct Capability(u32);

impl Clone for Capability {
    fn clone(&self) -> Self {
        // increase the reference count in the host table
        unsafe { abi::table::inc_ref(self.0) }
        Capability(self.0)
    }
}

impl Drop for Capability {
    fn drop(&mut self) {
        // decrease the reference count in the host table
        unsafe { abi::table::dec_ref(self.0) }
    }
}

impl Capability {
    /// Allow capabilities to be initialized from outside of this crate.
    pub const unsafe fn new_raw(handle: u32) -> Self {
        Capability(handle)
    }

    /// Sends a type, serialized as JSON, to this capability.
    pub fn send(&self, data: &impl Serialize, caps: &[&Capability]) {
        let json_msg = serde_json::to_string(data).unwrap();
        let bytes_msg = json_msg.into_bytes();
        self.send_raw(&bytes_msg, &caps);
    }

    /// Sends a raw message to this capability.
    pub fn send_raw(&self, data: &[u8], caps: &[&Capability]) {
        let caps: Vec<u32> = caps.iter().map(|cap| (*cap).borrow().0).collect();
        unsafe {
            abi::table::send(
                self.0,
                data.as_ptr() as u32,
                data.len() as u32,
                caps.as_ptr() as u32,
                caps.len() as u32,
            );
        }
    }

    /// Kills this capability.
    pub fn kill(&self) {
        unsafe { abi::table::kill(self.0) }
    }

    /// Demotes this capability to a capability with fewer permissions.
    pub fn demote(&self, new_perms: Permissions) -> Capability {
        let handle = unsafe { abi::table::demote(self.0, new_perms.bits()) };
        Capability(handle)
    }

    /// Gets the permission flags for this capability.
    pub fn get_flags(&self) -> Permissions {
        Permissions::from_bits_retain(unsafe { abi::table::get_permissions(self.0) })
    }
}

/// A signal.
#[derive(Clone, Debug)]
pub enum Signal {
    /// A down signal. Sent when a monitored capability's route is closed.
    Down {
        /// A capability to the monitored route with no permissions.
        subject: Capability,
    },

    /// A [Message] signal.
    Message(Message),
}

impl Signal {
    unsafe fn from_handle(handle: u32) -> Self {
        let kind = abi::mailbox::get_signal_kind(handle);

        let Ok(kind) = SignalKind::try_from(kind) else {
            panic!("unknown signal kind {}", kind);
        };

        let signal = match kind {
            SignalKind::Message => Signal::Message(Message::load_from_handle(handle)),
            SignalKind::Down => {
                let handle = abi::mailbox::get_down_capability(handle);
                let subject = Capability(handle);
                Signal::Down { subject }
            }
        };

        abi::mailbox::destroy_signal(handle);

        signal
    }
}

/// An un-closeable mailbox that receives signals from the parent of this process.
pub static PARENT: Mailbox = Mailbox(0);

/// A receiver of signals.
///
/// Make a capability to this mailbox using [Mailbox::make_capability] and send
/// it to other processes to allow other processes to interact with it.
///
/// If a mailbox is destroyed, it revokes the permission to kill this process
/// using a capability to the destroyed mailbox.
pub struct Mailbox(u32);

impl Drop for Mailbox {
    fn drop(&mut self) {
        // free this mailbox handle from the host API
        unsafe { abi::mailbox::destroy(self.0) }
    }
}

impl Mailbox {
    /// Creates a fresh mailbox with no capabilities to it.
    pub fn new() -> Self {
        let handle = unsafe { abi::mailbox::create() };
        Self(handle)
    }

    /// Make a capability to this mailbox with the given permission flags.
    pub fn make_capability(&self, perms: Permissions) -> Capability {
        let handle = unsafe { abi::mailbox::make_capability(self.0, perms.bits()) };
        Capability(handle)
    }

    /// Observe a subject capability for when it becomes unavailable.
    ///
    /// When it does, this mailbox will receive [Signal::Down] with a
    /// capability equivalent to the subject's but with no permissions.
    pub fn monitor(&self, subject: &Capability) {
        unsafe { abi::mailbox::monitor(self.0, subject.0) }
    }

    /// Wait for this mailbox to receive a [Signal].
    pub fn recv_signal(&self) -> Signal {
        unsafe {
            let handle = abi::mailbox::recv(self.0);
            Signal::from_handle(handle)
        }
    }

    /// Waits for one of many mailboxes to receive a signal.
    pub fn poll(mailboxes: &[&Self]) -> (usize, Signal) {
        let handles: Vec<_> = mailboxes.iter().map(|mb| mb.0).collect();
        let ptr = handles.as_ptr() as u32;
        let len = handles.len() as u32;
        let result = unsafe { abi::mailbox::poll(ptr, len) };
        let index = (result >> 32) as usize;
        let signal = unsafe { Signal::from_handle(result as u32) };
        (index, signal)
    }

    /// Receives a JSON message. Panics if the next signal isn't a message or
    /// if deserialization fails.
    pub fn recv<T>(&self) -> (T, Vec<Capability>)
    where
        T: for<'a> Deserialize<'a>,
    {
        let (bytes_data, caps) = self.recv_raw();
        let json_data = serde_json::from_slice(&bytes_data).unwrap();
        (json_data, caps)
    }

    /// Receives a raw bytes message. Panics if the next signal isn't a message or
    /// if deserialization fails.
    pub fn recv_raw(&self) -> (Vec<u8>, Vec<Capability>) {
        let signal = self.recv_signal();

        let Signal::Message(msg) = signal else {
            panic!("expected message, received {:?}", signal);
        };

        (msg.data, msg.caps)
    }

    /// Check if this mailbox has received any signals without waiting.
    pub fn try_recv_signal(&self) -> Option<Signal> {
        unsafe {
            let handle = abi::mailbox::try_recv(self.0);

            if handle == u32::MAX {
                None
            } else {
                Some(Signal::from_handle(handle))
            }
        }
    }

    /// Check if this mailbox has received any signals without waiting
    /// and return None if signal was not a message or it was down.
    pub fn try_recv_raw(&self) -> Option<(Vec<u8>, Vec<Capability>)> {
        let signal = self.try_recv_signal();

        match signal {
            Some(Signal::Message(msg)) => Some((msg.data, msg.caps)),
            Some(Signal::Down { subject }) => {
                panic!("received down signal on subject {:?}", subject)
            }
            None => None,
        }
    }

    /// Check if this mailbox has received any signals without waiting
    /// and return None if there was either no signal, it was down,
    /// or there was an error while processing json.
    pub fn try_recv<T>(&self) -> Option<(T, Vec<Capability>)>
    where
        T: for<'a> Deserialize<'a>,
    {
        let msg = self.try_recv_raw()?;

        let data = serde_json::from_slice(&msg.0).unwrap();

        Some((data, msg.1))
    }
}

/// A message that has been received from another process.
#[derive(Clone, Debug)]
pub struct Message {
    /// The raw data payload of this message.
    pub data: Vec<u8>,

    /// The list of capabilities that were transferred in this message.
    pub caps: Vec<Capability>,
}

impl Message {
    /// Loads a message signal by its handle.
    unsafe fn load_from_handle(handle: u32) -> Self {
        let data_len = abi::mailbox::get_message_data_len(handle) as usize;
        let mut data = Vec::with_capacity(data_len);
        data.set_len(data_len);
        abi::mailbox::get_message_data(handle, data.as_ptr() as u32);

        let caps_num = abi::mailbox::get_message_caps_num(handle) as usize;
        let mut caps = Vec::with_capacity(caps_num);
        caps.set_len(caps_num);
        abi::mailbox::get_message_caps(handle, caps.as_ptr() as u32);

        Self { data, caps }
    }
}

/// A loaded lump.
#[derive(Debug)]
pub struct Lump(u32);

impl Drop for Lump {
    fn drop(&mut self) {
        unsafe { abi::lump::free(self.0) }
    }
}

impl Lump {
    /// Loads a new lump directly from in-process bytes.
    pub fn load_raw(data: &[u8]) -> Self {
        unsafe {
            let ptr = data.as_ptr() as u32;
            let len = data.len() as u32;
            let handle = abi::lump::load(ptr, len);
            Self(handle)
        }
    }

    /// Loads a JSON-encoded lump from a serializable data type.
    pub fn load(data: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(data).unwrap();
        Self::load_raw(&bytes)
    }

    /// Loads a lump from the ID of an already existing lump.
    pub fn load_by_id(id: &LumpId) -> Self {
        unsafe {
            let handle = abi::lump::load_by_id(id as *const LumpId as u32);
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

/// Log a message.
pub fn log(level: ProcessLogLevel, module: &str, content: &str) {
    let level = level.into();
    let (module_ptr, module_len) = abi_string(module);
    let (content_ptr, content_len) = abi_string(content);
    unsafe { abi::log::log(level, module_ptr, module_len, content_ptr, content_len) }
}

#[allow(clashing_extern_declarations)]
mod abi {
    pub mod log {
        #[link(wasm_import_module = "hearth::log")]
        extern "C" {
            pub fn log(
                level: u32,
                module_ptr: u32,
                module_len: u32,
                content_ptr: u32,
                content_len: u32,
            );
        }
    }

    pub mod lump {
        #[link(wasm_import_module = "hearth::lump")]
        extern "C" {
            pub fn this_lump(ptr: u32);
            pub fn load_by_id(id_ptr: u32) -> u32;
            pub fn load(ptr: u32, len: u32) -> u32;
            pub fn get_id(handle: u32, id_ptr: u32);
            pub fn get_len(handle: u32) -> u32;
            pub fn get_data(handle: u32, ptr: u32);
            pub fn free(handle: u32);
        }
    }

    pub mod table {
        #[link(wasm_import_module = "hearth::table")]
        extern "C" {
            pub fn inc_ref(handle: u32);
            pub fn dec_ref(handle: u32);
            pub fn get_permissions(handle: u32) -> u32;
            pub fn demote(handle: u32, perms: u32) -> u32;
            pub fn send(handle: u32, data_ptr: u32, data_len: u32, caps_ptr: u32, caps_len: u32);
            pub fn kill(handle: u32);
        }
    }

    pub mod mailbox {
        #[link(wasm_import_module = "hearth::mailbox")]
        extern "C" {
            pub fn create() -> u32;
            pub fn destroy(handle: u32);
            pub fn make_capability(handle: u32, perms: u32) -> u32;
            pub fn monitor(mailbox: u32, cap: u32);
            pub fn recv(handle: u32) -> u32;
            pub fn try_recv(handle: u32) -> u32;
            pub fn poll(handles_ptr: u32, handles_len: u32) -> u64;
            pub fn destroy_signal(handle: u32);
            pub fn get_signal_kind(handle: u32) -> u32;
            pub fn get_down_capability(handle: u32) -> u32;
            pub fn get_message_data_len(handle: u32) -> u32;
            pub fn get_message_data(handle: u32, dst_ptr: u32);
            pub fn get_message_caps_num(handle: u32) -> u32;
            pub fn get_message_caps(handle: u32, dst_ptr: u32);
        }
    }
}

/// Exports this WebAssembly module's process metadata using the calling Cargo
/// package's configuration.
///
/// Use this macro exactly once in the top level of a guest Wasm module's Rust
/// crate.
///
/// Uses the following package settings as fields of the process metadata:
/// - `name`: the name of the package.
/// - `description`: a short description of the package's usage.
/// - `authors`: a list of authors that have contributed to the package.
/// - `repository`: a link to the home source repository of the package.
/// - `homepage`: a link to the package's homepage.
/// - `license`: an SPDX license identifier for the package's source code.
///
/// See [Cargo's documentation](https://doc.rust-lang.org/cargo/reference/manifest.html#the-package-section) for more info.
#[macro_export]
macro_rules! export_metadata {
    () => {
        #[no_mangle]
        extern "C" fn _hearth_metadata() {
            // define the ABI functions in the function since we only use them here
            #[link(wasm_import_module = "hearth::metadata")]
            extern "C" {
                fn set_name(ptr: u32, len: u32);
                fn set_description(ptr: u32, len: u32);
                fn add_author(ptr: u32, len: u32);
                fn set_repository(ptr: u32, len: u32);
                fn set_homepage(ptr: u32, len: u32);
                fn set_license(ptr: u32, len: u32);
            }

            // helper function to return Some(str) when str is not empty and None if empty
            let some_or_empty = |str: &str| {
                if str.is_empty() {
                    None
                } else {
                    let bytes = str.as_bytes();
                    let ptr = bytes.as_ptr() as u32;
                    let len = bytes.len() as u32;
                    Some((ptr, len))
                }
            };

            if let Some((ptr, len)) = some_or_empty(env!("CARGO_PKG_NAME")) {
                unsafe { set_name(ptr, len) };
            }

            if let Some((ptr, len)) = some_or_empty(env!("CARGO_PKG_DESCRIPTION")) {
                unsafe { set_description(ptr, len) };
            }

            let authors = env!("CARGO_PKG_AUTHORS");
            if !authors.is_empty() {
                // authors are split by ':' characters in the environment variable
                for author in authors.split(':') {
                    let bytes = author.as_bytes();
                    let ptr = bytes.as_ptr() as u32;
                    let len = bytes.len() as u32;
                    unsafe { add_author(ptr, len) };
                }
            }

            if let Some((ptr, len)) = some_or_empty(env!("CARGO_PKG_REPOSITORY")) {
                unsafe { set_repository(ptr, len) };
            }

            if let Some((ptr, len)) = some_or_empty(env!("CARGO_PKG_HOMEPAGE")) {
                unsafe { set_homepage(ptr, len) };
            }

            if let Some((ptr, len)) = some_or_empty(env!("CARGO_PKG_LICENSE")) {
                unsafe { set_license(ptr, len) };
            }
        }
    };
}

#[no_mangle]
extern "C" fn _hearth_init() {
    // set panic handler that prints error to log
    std::panic::set_hook(Box::new(|info| {
        // references default_hook() from
        // https://doc.rust-lang.org/src/std/panicking.rs.html
        let location = info.location().unwrap();

        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
            *s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            &s[..]
        } else {
            "Box<dyn Any>"
        };

        let log_message = format!("panicked at '{msg}', {location}");
        log(ProcessLogLevel::Error, "panic", &log_message);
    }));

    // initialize tracing subscriber
    let subscriber = ProcessSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).expect("failed to set subscriber");
}

#[no_mangle]
extern "C" fn _hearth_spawn_by_index(function: u32) {
    // unsafely get a function pointer directly from the Wasm function index
    let function: fn() = unsafe { std::mem::transmute(function as usize) };
    function();
}
