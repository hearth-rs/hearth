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

use std::collections::HashMap;

use hearth_guest::Capability;
use kindling_host::{prelude::*, registry::Registry};
use kindling_utils::registry::*;
use petgraph::{algo::toposort, prelude::DiGraph};
use serde::Deserialize;

hearth_guest::export_metadata!();

/// The subpath within the filesystem root where services are scanned.
const SEARCH_DIR: &str = "init";

/// A persistent service container object.
pub struct Service {
    /// A capability to this process, stays as `None` until this process is started.
    pub process: Option<Capability>,

    name: String,
    config: ServiceConfig,
}

impl Service {
    pub fn new(name: String, config: ServiceConfig) -> Self {
        Self {
            name,
            process: None,
            config,
        }
    }

    pub fn spawn(&mut self, registry: Option<Registry>) -> Capability {
        let lump = get_file(&format!("{}/{}/service.wasm", SEARCH_DIR, self.name))
            .expect("WASM module not found");
        let cap = spawn_mod(lump, registry.map(|x| x.as_ref().to_owned()));
        self.process = Some(cap.to_owned());
        cap
    }
}

#[no_mangle]
pub extern "C" fn run() {
    info!("Hello world!");

    // first of all, enumerate available native services
    let native_services = REGISTRY.list_services();

    // add all guest services into a dependency graph structure
    let mut graph = DiGraph::<Service, ()>::new();

    // map of service names to indices within the graph
    let mut names_to_idxs = HashMap::new();

    // list all service directories
    for file in list_files(SEARCH_DIR).unwrap() {
        info!("file: {}", file.name);

        // attempt to parse config
        let Some(config) = get_config(&file.name) else {
            error!("Failed to get config");
            continue;
        };

        info!("config: {:?}", config);

        // add service node to graph
        let name = file.name;
        let service = Service::new(name.clone(), config);
        let idx = graph.add_node(service);
        names_to_idxs.insert(name, idx);
    }

    // add dependency edges to graph
    for idx in graph.node_indices() {
        let node = graph.node_weight(idx).unwrap();
        let name = node.name.clone();
        info!("Collecting dependencies of service \'{name}\'");

        // track whether this service has any missing deps
        let mut remove = false;

        // iterate all needed deps
        for dep in node.config.dependencies.need.clone() {
            match names_to_idxs.get(&dep.clone()) {
                // is this service an existing guest process?
                Some(dep_idx) => {
                    graph.add_edge(*dep_idx, idx, ());
                }
                // guest service not found
                None => {
                    // check if the service is native
                    // if the service is native, we skip adding this edge, and
                    // its capability will be retrieved during service startup
                    if !native_services.contains(&dep) {
                        // if it isn't, this dep is missing
                        remove = true;
                        error!("Dependency \'{dep}\' not found");
                    }
                }
            };
        }

        // if this service can't start, remove the service from the graph
        if remove {
            info!("Service \'{name}\' will not be spawned");
            graph.remove_node(idx);
            names_to_idxs.remove(&name);
        }
    }

    // order service graph so that dependencies start before dependents
    // panic may occur here if the dep graph has a cycle
    let sorted_services = toposort(&graph, None).unwrap();

    // create a cache of service names to their started capabilities
    let mut names_to_caps: HashMap<String, Capability> = HashMap::new();

    // populate the cache with caps to the native services
    for service in native_services {
        let cap = REGISTRY.get_service(&service).unwrap();
        names_to_caps.insert(service, cap);
    }

    // start up all guest services in dependency order
    for idx in sorted_services {
        // get service data
        let service = graph.node_weight_mut(idx).unwrap();

        // create associated list of all deps' caps
        let mut deps = Vec::new();
        for dep in service.config.dependencies.need.clone() {
            // look up service cap (either guest or host)
            let cap = names_to_caps.get(&dep).unwrap().to_owned();
            deps.push((dep, cap));
        }

        // create a new registry with this service's deps
        let registry = Some(RegistryServer::spawn(deps));

        // spawn the service
        let cap = service.spawn(registry);

        // provide this service to its dependents
        names_to_caps.insert(service.name.clone(), cap);
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Dependencies {
    #[serde(default)]
    pub need: Vec<String>,

    #[serde(default)]
    pub milestone: Vec<String>,

    #[serde(default)]
    pub waits_for: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct License {
    pub name: String,
    pub file: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceConfig {
    #[serde(default)]
    pub dependencies: Dependencies,

    pub description: Option<String>,

    #[serde(default)]
    pub license: Vec<License>,

    #[serde(default)]
    pub targets: Vec<String>,
}

fn get_config(name: &str) -> Option<ServiceConfig> {
    let config_path = format!("{}/{}/service.toml", SEARCH_DIR, name);
    let config_data = read_file(&config_path).ok()?;
    let config_str = String::from_utf8(config_data).unwrap();
    toml::from_str(&config_str).ok()
}
