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

use serde::{Deserialize, Serialize};

pub use crate::Permissions;

/// A reason for the revocation or unlinking of a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum UnlinkReason {
    /// The process is no longer alive.
    Dead,

    /// The process is no longer accessible.
    Inaccessible,

    /// Access to the process has been revoked.
    AccessRevoked,
}

/// Types of messages relating to low-level capability operations between two peers.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum CapOperation {
    Local(LocalCapOperation),
    Remote(RemoteCapOperation),
}

/// Operations on local capabilities.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum LocalCapOperation {
    /// Declares a capability and its identifier.
    DeclareCap { id: u32, perms: Permissions },

    /// Revokes a capability.
    ///
    /// All operations on this capability become invalid when this operation
    /// is sent, but the capability ID will not be reused until
    /// [RemoteCapOperation::AcknowledgeRevocation] is received.
    RevokeCap { id: u32, reason: UnlinkReason },

    /// Sets an already-declared capability to be the "root cap".
    ///
    /// The root cap is the capability that each end of a network connection
    /// gives to the other end without prompt. Clients and servers exchange
    /// registries to each other upon connection. The IPC daemon gives IPC
    /// clients a root registry upon connection too, although the IPC client
    /// won't.
    ///
    /// This cap may be revoked like any other cap, so bear in mind.
    SetRootCap { id: u32 },
}

/// Operations on remote capabilities.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum RemoteCapOperation {
    /// Acknowledges that a capability has been revoked, freeing the ID for
    /// reuse.
    AcknowledgeRevocation { id: u32 },

    /// Communicates that a capability is no longer being used.
    ///
    /// Local cap operations may still may still be received using this
    /// capability and the sender of this operation must assume that the ID of
    /// this cap will stay in use until it is revoked.
    FreeCap { id: u32 },

    /// Sends a message to a remote capability.
    ///
    /// Ignored if the capability does not have [Permissions::SEND] set.
    Send {
        /// The remote capability to send a message to.
        ///
        /// Ignored if invalid or revoked.
        id: u32,

        /// The contents of the message.
        data: Vec<u8>,

        /// The local capabilities transferred in this message.
        caps: Vec<u32>,
    },

    /// Kills a remote capability.
    ///
    /// Ignored if the capability does not have [Permissions::KILL] set.
    Kill {
        /// The remote capability to kill.
        ///
        /// Ignored if invalid or revoked.
        id: u32,
    },
}
