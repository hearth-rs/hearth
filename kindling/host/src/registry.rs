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

use super::*;

use hearth_guest::{
    registry::{self, RegistryRequest, RegistryResponse},
    Capability,
};

/// A wrapper for capabilities implementing the [registry] protocol.
pub type Registry = RequestResponse<registry::RegistryRequest, registry::RegistryResponse>;

impl Registry {
    /// Gets a service by its name. Returns `None` if the service doesn't exist.
    pub fn get_service(&self, name: &str) -> Option<Capability> {
        let request = registry::RegistryRequest::Get {
            name: name.to_string(),
        };

        let (data, mut caps) = self.request(request, &[]);

        let registry::RegistryResponse::Get(present) = data else {
            panic!("failed to get service {:?}", name);
        };

        if present {
            Some(caps.remove(0))
        } else {
            None
        }
    }

    /// Lists all services in this registry.
    pub fn list_services(&self) -> Vec<String> {
        let (data, _) = self.request(RegistryRequest::List, &[]);
        let RegistryResponse::List(list) = data else {
            panic!("failed to list services");
        };
        list
    }
}

/// A capability to the registry that this process has base access to.
pub static REGISTRY: Registry = RequestResponse::new(unsafe { Capability::new_raw(0) });
