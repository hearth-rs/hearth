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

/// The default [store::ProcessStoreTrait] implementation.
pub type ProcessStore = store::ProcessStore<store::AnyProcess>;

/// The default process registry using [ProcessStore].
pub type Registry = registry::Registry<ProcessStore>;

/// The default process factory using [ProcessStore].
pub type ProcessFactory = factory::ProcessFactory<ProcessStore>;

/// The default local process using [ProcessStore].
pub type Process = factory::Process<ProcessStore>;

pub mod context;
pub mod factory;
pub mod local;
pub mod registry;
pub mod rpc;
pub mod store;