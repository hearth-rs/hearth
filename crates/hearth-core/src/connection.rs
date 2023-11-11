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

//! Implements peer-to-peer capability exchange code for remote processes.
//!
//! Currently unimplemented.

// TODO get rid of this when connections are implemented
#![allow(unused)]

use std::{collections::HashMap, sync::Arc};

use flue::{CapabilityRef, Mailbox, MailboxGroup, OwnedCapability, PostOffice, Table};
use flume::{Receiver, Sender};
use hearth_types::protocol::{CapOperation, LocalCapOperation, RemoteCapOperation};
use ouroboros::self_referencing;
use parking_lot::Mutex;
use tokio::sync::oneshot;

pub type RootCapSender = oneshot::Sender<OwnedCapability>;

#[self_referencing]
struct Import<'a> {
    conn: &'a Connection,

    group: MailboxGroup<'a>,

    #[borrows(group)]
    #[covariant]
    mb: Mailbox<'this>,
}

struct Export<'a> {
    cap: CapabilityRef<'a>,
    revoked: bool,
}

struct Exports<'a> {
    table: &'a Table,
    inner: Mutex<HashMap<u32, Export<'a>>>,
}

/// A data structure implementing the capability exchange protocol.
///
/// Currently unimplemented.
#[self_referencing]
pub struct Connection {
    table: Table,

    #[borrows(table)]
    #[not_covariant]
    exports: Exports<'this>,
}

impl Connection {
    /// Initializes a new connection.
    ///
    /// `op_rx` is the channel receiver used to receive incoming [CapOperation]
    /// messages on this connection. `op_tx` is the channel sender used to send
    /// outgoing [CapOperation]s.
    pub fn begin(
        post: Arc<PostOffice>,
        op_rx: Receiver<CapOperation>,
        op_tx: Sender<CapOperation>,
        on_root_cap: Option<RootCapSender>,
    ) -> Arc<Self> {
        let conn = Connection::new(Table::new(post), |table| Exports {
            table,
            inner: Default::default(),
        });

        let conn = Arc::new(conn);

        conn
    }

    /// Exports a capability through this connection.
    pub fn export(&self, cap: OwnedCapability) -> u32 {
        self.with_exports(|exports| {
            let table = exports.table;
            let mut exports = exports.inner.lock();
            let handle = table.import_owned(cap).unwrap();
            let id: u32 = handle.0.try_into().unwrap();

            if exports.contains_key(&id) {
                // cap is already exported, so drop this reference
                table.dec_ref(handle).unwrap();
            } else {
                // cap needs to be exported
                let cap = table.wrap_handle(handle).unwrap();
                let perms = cap.get_permissions().bits();
                let perms = hearth_types::Permissions::from_bits_retain(perms);
                let op = LocalCapOperation::DeclareCap { id, perms };
                let revoked = false;
                let export = Export { cap, revoked };
                exports.insert(id, export);
                self.send_local_op(op);
            }

            id
        })
    }

    /// Exports a capability as this side of the connection's root cap.
    pub fn export_root(&self, cap: OwnedCapability) {
        let id = self.export(cap);
        self.send_local_op(LocalCapOperation::SetRootCap { id });
    }

    pub fn on_op(self: &Arc<Self>, op: CapOperation) {
        match op {
            CapOperation::Local(op) => self.on_local_op(op),
            CapOperation::Remote(op) => self.on_remote_op(op),
        }
    }

    fn on_local_op(self: &Arc<Self>, op: LocalCapOperation) {
        use LocalCapOperation::*;
        match op {
            DeclareCap { id, perms } => todo!(),
            RevokeCap { id, reason } => todo!(),
            SetRootCap { id } => todo!(),
        }
    }

    fn on_remote_op(&self, op: RemoteCapOperation) {
        use RemoteCapOperation::*;
        match op {
            AcknowledgeRevocation { id } => {
                self.with_exports(|exports| {
                    let mut exports = exports.inner.lock();
                    if let Some(export) = exports.get(&id) {
                        if export.revoked {
                            exports.remove(&id);
                        }
                    }
                });
            }
            FreeCap { id } => todo!(),
            Send { id, data, caps } => self.with_export(id, |cap| {}),
            Kill { id } => self.with_export(id, |cap| {
                let _ = cap.kill();
            }),
        }
    }

    fn with_export(&self, id: u32, mut cb: impl FnMut(&CapabilityRef<'_>)) {
        self.with_exports(|exports| {
            let inner = exports.inner.lock();
            if let Some(cap) = inner.get(&id) {
                if !cap.revoked {
                    cb(&cap.cap);
                }
            }
        });
    }

    fn send_local_op(&self, op: LocalCapOperation) {}

    fn send_remote_op(&self, op: RemoteCapOperation) {}
}
