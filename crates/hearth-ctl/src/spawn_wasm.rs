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
use hearth_ipc::RemoteProcess;
use hearth_rpc::hearth_types::wasm::WasmSpawnInfo;
use hearth_rpc::remoc::robj::lazy_blob::LazyBlob;
use hearth_rpc::*;
use hearth_types::*;
use std::fs::read;
use std::path::Path;

/// Spawns a Web Assembly module on a specific peer
#[derive(Debug, Parser)]
pub struct SpawnWasm {
    #[clap(short, long)]
    pub peer: Option<u32>,
    pub file: String,
}

impl SpawnWasm {
    pub async fn run(self, daemon: DaemonOffer) {
        let peer = self.peer.map(|x| PeerId(x)).unwrap_or(daemon.peer_id);
        let peer_api = daemon.peer_provider.find_peer(peer).await.unwrap();
        let process_store = peer_api.get_process_store().await.unwrap();
        let path = Path::new(&self.file);
        let pid = process_store
            .follow_service_list()
            .await
            .unwrap()
            .take_initial()
            .unwrap()
            .get("hearth.cognito.WasmProcessSpawner")
            .copied()
            .expect("Peer is not running WebAssembly");
        let process = RemoteProcess::new(&daemon, ProcessInfo {}).await.unwrap();
        let lump_id = peer_api
            .get_lump_store()
            .await
            .unwrap()
            .upload_lump(
                None,
                LazyBlob::new(read(path).expect("No file at path").into()),
            )
            .await
            .unwrap();

        let wasm_spawn_info = WasmSpawnInfo {
            lump: lump_id,
            entrypoint: None,
        };

        process
            .outgoing
            .send(Message {
                pid: ProcessId::from_peer_process(peer, pid),
                data: serde_json::to_vec(&wasm_spawn_info).unwrap(),
            })
            .await
            .unwrap();

        // necessary to flush the message send; remove when waiting for the returned PID
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
