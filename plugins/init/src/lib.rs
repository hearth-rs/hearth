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

use std::{path::PathBuf, sync::Arc};

use hearth_runtime::{
    async_trait, cargo_process_metadata,
    flue::{OwnedCapability, Permissions, TableSignal},
    hearth_schema::{registry::RegistryRequest, wasm::WasmSpawnInfo},
    process::{Process, ProcessMetadata},
    runtime::{Plugin, Runtime, RuntimeBuilder},
    tokio::{spawn, sync::oneshot::Sender},
    utils::ProcessRunner,
};
use tracing::{debug, warn};

struct Hook {
    service: String,
    callback: Sender<OwnedCapability>,
}

#[async_trait]
impl ProcessRunner for Hook {
    async fn run(mut self, label: String, _runtime: Arc<Runtime>, ctx: &Process) {
        // handles incoming signals. returns None until a valid hook message arrives.
        let on_recv = |signal: TableSignal<'_>| {
            let TableSignal::Message { data: _, caps } = signal else {
                tracing::error!("expected message, got {:?}", signal);
                return None;
            };

            let Some(init_cap) = caps.first() else {
                warn!("{label} hook received message with no caps");
                return None;
            };

            Some(*init_cap)
        };

        while let Some(hook) = ctx.borrow_parent().recv(on_recv).await {
            // if we got a valid hook message, handle it and quit.
            if let Some(init_cap) = hook {
                let cap = ctx.borrow_table().get_owned(init_cap).unwrap();
                let _ = self.callback.send(cap);
                return;
            }
        }
    }
}

pub struct InitPlugin {
    init_path: PathBuf,
    hooks: Vec<Hook>,
}

impl Plugin for InitPlugin {
    fn finalize(self, builder: &mut RuntimeBuilder) {
        for hook in self.hooks {
            let mut meta = cargo_process_metadata!();
            meta.name = Some(hook.service.clone());
            meta.description = Some("An init hook. Send a message with no data and a single capability to initialize it.".to_string());

            builder.add_service(hook.service.clone(), meta, hook);
        }

        builder.add_runner(move |runtime| {
            spawn(async move {
                debug!("Loading init system module");
                let wasm_data = std::fs::read(self.init_path.clone()).unwrap();
                let wasm_lump = runtime.lump_store.add_lump(wasm_data.into()).await;

                let spawn_info = WasmSpawnInfo {
                    lump: wasm_lump,
                    entrypoint: None,
                };

                debug!("Running init system");
                let mut meta = cargo_process_metadata!();
                meta.name = Some("init system parent".to_string());

                let parent = runtime.process_factory.spawn(meta);
                let response = parent.borrow_group().create_mailbox().unwrap();
                let response_cap = response.export(Permissions::SEND).unwrap();

                let perms = Permissions::SEND | Permissions::MONITOR;
                let registry = runtime
                    .registry
                    .borrow_parent()
                    .export_to(perms, parent.borrow_table())
                    .unwrap();

                let request = RegistryRequest::Get {
                    name: "hearth.wasm.WasmProcessSpawner".to_string(),
                };

                registry
                    .send(&serde_json::to_vec(&request).unwrap(), &[&response_cap])
                    .await
                    .unwrap();

                let spawner = response
                    .recv(|signal| {
                        let TableSignal::Message { mut caps, .. } = signal else {
                            panic!("expected message, got {:?}", signal);
                        };

                        caps.remove(0)
                    })
                    .await
                    .unwrap();

                let spawner = parent.borrow_table().wrap_handle(spawner).unwrap();

                spawner
                    .send(
                        &serde_json::to_vec(&spawn_info).unwrap(),
                        &[&response_cap, &registry],
                    )
                    .await
                    .unwrap();
            });
        });
    }
}

impl InitPlugin {
    pub fn new(init_path: PathBuf) -> Self {
        Self {
            init_path,
            hooks: Vec::new(),
        }
    }

    pub fn add_hook(&mut self, service: String, callback: Sender<OwnedCapability>) {
        self.hooks.push(Hook { service, callback });
    }
}
