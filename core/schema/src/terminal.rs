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

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FactoryError {
    /// The request has failed to parse.
    ParseError,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TerminalState {
    pub position: Vec3,
    pub orientation: Quat,
    pub half_size: Vec2,
    pub opacity: f32,
    pub padding: Vec2,
    pub units_per_em: f32,
    pub colors: HashMap<usize, Color>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum TerminalUpdate {
    Quit,
    Input(String),
    State(TerminalState),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FactoryRequest {
    CreateTerminal(TerminalState),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FactorySuccess {
    /// The first returned capability is to the new terminal, which receives [TerminalUpdates][TerminalUpdate].
    Terminal,
}

pub type FactoryResponse = Result<FactorySuccess, FactoryError>;
