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

use crate::LumpId;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Error {
    NotFound,
    PermissionDenied,
    IsADirectory,
    NotADirectory,
    DirectoryTraversal,
    InvalidTarget,
    InvalidRequest,
    Other(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RequestKind {
    Get,
    List,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    pub target: String,
    pub kind: RequestKind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileInfo {
    pub name: String,
    // TODO more file properties like size or last modified?
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Success {
    Get(LumpId),
    List(Vec<FileInfo>),
}

pub type Response = Result<Success, Error>;
