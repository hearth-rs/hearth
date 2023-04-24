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

use clap::Parser;
use hearth_rpc::*;

use crate::MaybeLocalPID;

/// Kill a process
#[derive(Debug, Parser)]
pub struct Kill {
    /// Take either a global or local process id
    pub process: MaybeLocalPID,
}

impl Kill {
    pub async fn run(self, daemon: DaemonOffer) {
        let (peer, local_pid) = self.process.to_global_pid(daemon.peer_id).split();

        daemon
            .peer_provider
            .find_peer(peer)
            .await
            .expect("Peer not found")
            .get_process_store()
            .await
            .expect("Peer does not have a process store")
            .find_process(local_pid)
            .await
            .expect("Could not find process")
            .kill()
            .await
            .unwrap();
    }
}