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

//! Hearth runtime construction and the plugin interface.
//!
//! To get started, call [RuntimeBuilder::new] to start building a runtime,
//! then add plugins, runners, or asset loaders to the builder. When finished,
//! call [RuntimeBuilder::run] to start up the configured runtime.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use hearth_rpc::remoc::rtc::ServerShared;
use hearth_rpc::*;
use hearth_types::PeerId;
use remoc::rtc::async_trait;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

use crate::asset::{AssetLoader, AssetStore};
use crate::lump::LumpStoreImpl;
use crate::process::{Process, ProcessFactoryImpl, ProcessStoreImpl};

/// Interface trait for plugins to the Hearth runtime.
///
/// Each plugin first builds onto a runtime using its `build` function and an
/// in-progress [RuntimeBuilder]. After all plugins are added, the runtime
/// starts, and the `run` method is called with a handle to the new runtime.
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Builds a runtime using this plugin. See [RuntimeBuilder] for more info.
    fn build(&mut self, builder: &mut RuntimeBuilder);

    /// Runs this plugin using an instantiated [Runtime].
    async fn run(&mut self, runtime: Arc<Runtime>);
}

#[allow(clippy::type_complexity)]
struct PluginWrapper {
    plugin: Box<dyn Any>,
    runner: Box<dyn FnOnce(Box<dyn Any>, Arc<Runtime>) -> JoinHandle<()>>,
}

/// Builder struct for a single Hearth [Runtime].
#[allow(clippy::type_complexity)]
pub struct RuntimeBuilder {
    config_file: toml::Table,
    plugins: HashMap<TypeId, PluginWrapper>,
    runners: Vec<Box<dyn FnOnce(Arc<Runtime>) -> JoinHandle<()>>>,
    services: HashSet<String>,
    lump_store: Arc<LumpStoreImpl>,
    asset_store: AssetStore,
}

impl RuntimeBuilder {
    /// Creates a new [RuntimeBuilder] with nothing loaded.
    pub fn new(config_file: toml::Table) -> Self {
        let lump_store = Arc::new(LumpStoreImpl::new());
        let asset_store = AssetStore::new(lump_store.clone());

        Self {
            config_file,
            plugins: Default::default(),
            runners: Default::default(),
            services: Default::default(),
            lump_store,
            asset_store,
        }
    }

    /// Loads a configuration value from a table in the config file.
    pub fn load_config<T: serde::de::DeserializeOwned>(&self, table: &str) -> anyhow::Result<T> {
        let value = self
            .config_file
            .get(table)
            .ok_or_else(|| anyhow::anyhow!("No table '{}' in config file", table))?
            .to_owned();

        T::deserialize(value).map_err(|err| {
            anyhow::anyhow!("Failed to deserialize '{}' in config: {:?}", table, err)
        })
    }

    /// Adds a plugin to the runtime.
    ///
    /// Plugins may use their [Plugin::build] method to add other plugins,
    /// asset loaders, runners, or anything else.
    pub fn add_plugin<T: Plugin>(&mut self, mut plugin: T) -> &mut Self {
        let name = std::any::type_name::<T>();
        debug!("Adding {} plugin", name);

        let id = plugin.type_id();
        if self.plugins.contains_key(&id) {
            warn!("Attempted to add {} plugin twice", name);
            return self;
        }

        plugin.build(self);

        self.plugins.insert(
            id,
            PluginWrapper {
                plugin: Box::new(plugin),
                runner: Box::new(move |plugin, runtime| {
                    let mut plugin = plugin.downcast::<T>().unwrap();
                    tokio::spawn(async move {
                        debug!("Running {} plugin", name);
                        plugin.run(runtime).await;
                    })
                }),
            },
        );

        self
    }

    /// Adds a runner to the runtime.
    ///
    /// Runners are simple async functions that are spawned when the runtime is
    /// started and are passed a handle to the new runtime. This may be used
    /// for long-running event processing code or other functionality that
    /// lasts the runtime's lifetime.
    pub fn add_runner<F, R>(&mut self, cb: F) -> &mut Self
    where
        F: FnOnce(Arc<Runtime>) -> R + Send + Sync + 'static,
        R: Future<Output = ()> + Send,
    {
        self.runners.push(Box::new(|runner| {
            tokio::spawn(async move {
                cb(runner).await;
            })
        }));

        self
    }

    /// Adds a service.
    ///
    /// Logs an error and skips adding the service if the service name is
    /// taken.
    ///
    /// Behind the scenes this creates a runner that spawns the process and
    /// registers it as a service.
    pub fn add_service(&mut self, name: String, process: impl Process) -> &mut Self {
        if self.services.contains(&name) {
            error!("Service name {} is taken", name);
            return self;
        }

        self.services.insert(name.clone());
        self.runners.push(Box::new(move |runtime| {
            tokio::spawn(async move {
                debug!("Spawning '{}' service", name);
                let pid = runtime.process_store.spawn(process).await;
                let result = runtime.process_store.register_service(pid, name).await;
                match result {
                    Ok(_) => {}
                    Err(err) => {
                        error!("Service registration error: {:?}", err);
                    }
                }
            })
        }));

        self
    }

    /// Adds a new asset loader.
    ///
    /// Logs an error event if the asset loader has already been added.
    pub fn add_asset_loader(&mut self, loader: impl AssetLoader) -> &mut Self {
        self.asset_store.add_loader(loader);
        self
    }

    /// Retrieves a reference to a plugin that has already been added.
    ///
    /// This function is intended to be used for dependencies of plugins, where
    /// a plugin may need to look up or modify the contents of a previously-
    /// added plugin. Using this function saves the code building the runtime
    /// the trouble of manually passing runtimes to other runtimes as
    /// dependencies.
    pub fn get_plugin<T: Plugin>(&self) -> Option<&T> {
        self.plugins
            .get(&TypeId::of::<T>())
            .and_then(|p| p.plugin.downcast_ref())
    }

    /// Retrieves a mutable reference to a plugin that has already been added.
    ///
    /// Mutable version of [Self::get_plugin].
    pub fn get_plugin_mut<T: Plugin>(&mut self) -> Option<&mut T> {
        self.plugins
            .get_mut(&TypeId::of::<T>())
            .and_then(|p| p.plugin.downcast_mut())
    }

    /// Consumes this builder and starts up the full [Runtime].
    ///
    /// This returns a shared pointer to the new runtime, as well as all of the
    /// [JoinHandles][JoinHandle] for the launched runners and plugins.
    pub fn run(self, config: RuntimeConfig) -> (Arc<Runtime>, Vec<JoinHandle<()>>) {
        debug!("Spawning lump store server");
        let lump_store = self.lump_store;
        let (lump_store_server, lump_store_client) =
            LumpStoreServerShared::new(lump_store.clone(), 1024);
        tokio::spawn(async move {
            lump_store_server.serve(true).await;
        });

        debug!("Spawning process store server");
        let process_store = ProcessStoreImpl::new(config.this_peer);
        let (process_store_server, process_store_client) =
            ProcessStoreServerShared::new(process_store.clone(), 1024);
        tokio::spawn(async move {
            process_store_server.serve(true).await;
        });

        debug!("Spawning process factory server");
        let process_factory = ProcessFactoryImpl::new(process_store.clone());
        let (process_factory_server, process_factory_client) =
            ProcessFactoryServerShared::new(Arc::new(process_factory), 1024);
        tokio::spawn(async move {
            process_factory_server.serve(true).await;
        });

        let runtime = Arc::new(Runtime {
            asset_store: Arc::new(self.asset_store),
            lump_store,
            lump_store_client,
            process_store,
            process_store_client,
            process_factory_client,
            config,
        });

        let mut join_handles = Vec::new();

        debug!("Running plugins");
        for (_id, wrapper) in self.plugins {
            let PluginWrapper { plugin, runner } = wrapper;
            let join = runner(plugin, runtime.clone());
            join_handles.push(join);
        }

        debug!("Running runners");
        for runner in self.runners {
            let join = runner(runtime.clone());
            join_handles.push(join);
        }

        (runtime, join_handles)
    }
}

/// Configuration info for a runtime.
pub struct RuntimeConfig {
    /// The provider to other peers on the network.
    pub peer_provider: PeerProviderClient,

    /// The ID of this peer.
    pub this_peer: PeerId,

    /// The [PeerInfo] for this peer.
    pub info: PeerInfo,
}

/// An instance of a single Hearth runtime.
///
/// This contains all of the resources that are used by plugins, processes,
/// and network peers. A runtime can be built and started using
/// [RuntimeBuilder].
///
/// Note that Hearth uses Tokio for all of its asynchronous
/// task execution and IO, so it's assumed that a Tokio runtime has already
/// been created.
pub struct Runtime {
    /// The configuration of this runtime.
    pub config: RuntimeConfig,

    //// The assets in this runtime.
    pub asset_store: Arc<AssetStore>,

    /// This runtime's lump store.
    pub lump_store: Arc<LumpStoreImpl>,

    /// A clone-able client to this runtime's lump store.
    pub lump_store_client: LumpStoreClient,

    /// This runtime's process store.
    pub process_store: Arc<ProcessStoreImpl>,

    /// A clone-able client to this runtime's process store.
    pub process_store_client: ProcessStoreClient,

    /// A clone-able client to the process store's factory.
    pub process_factory_client: ProcessFactoryClient,
}

#[async_trait]
impl PeerApi for Runtime {
    async fn get_info(&self) -> CallResult<PeerInfo> {
        Ok(self.config.info.clone())
    }

    async fn get_process_store(&self) -> CallResult<ProcessStoreClient> {
        Ok(self.process_store_client.clone())
    }

    async fn get_lump_store(&self) -> CallResult<LumpStoreClient> {
        Ok(self.lump_store_client.clone())
    }
}

impl Runtime {
    /// Spawns a new [PeerApiServer] for this runtime and returns a client to it.
    pub fn serve_peer_api(self: Arc<Self>) -> PeerApiClient {
        debug!("Serving runtime PeerApi");
        let (server, client) = PeerApiServerShared::new(self, 1024);
        tokio::spawn(async move {
            server.serve(true).await;
        });

        client
    }
}
