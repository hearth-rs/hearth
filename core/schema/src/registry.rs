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

/// A message schema for messages sent to a registry process. All variants require
/// that a reply cap is the first capability in the message.
///
/// Compliant registry processes will reply with a [RegistryResponse].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RegistryRequest {
    /// Gets a service by name. Returns [RegistryResponse::Get].
    Get { name: String },

    /// Registers the second capability in the message with the given name.
    /// Returns [RegistryResponse::Register].
    Register { name: String },

    /// Requests a list of all of the registered services. Returns
    /// [RegistryReponse::List].
    List,
}

/// A response to a [RegistryRequest].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RegistryResponse {
    /// If true, returns the service with the requested name with the first
    /// capability, if false, the service is unavailable and no cap is given.
    Get(bool),

    /// Returns one of the following:
    /// - `Some(true)`: the service has been successfully registered and there
    ///   was an old service present.
    /// - `Some(false)`: the service has been successfully registered and no
    ///   service has been replaced.
    /// - `None`: this registry is read-only and the service has not been
    ///   registered.
    Register(Option<bool>),

    /// Returns a list of the names of all services in this registry.
    List(Vec<String>),
}
