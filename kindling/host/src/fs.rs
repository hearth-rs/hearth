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
use core::panic;

use hearth_guest::{fs::*, Lump, LumpId};

lazy_static::lazy_static! {
    static ref FILESYSTEM: RequestResponse<Request, Response> =
        RequestResponse::expect_service("hearth.fs.Filesystem");
}

/// Get a LumpId of a file from a path.
pub fn get_file(path: &str) -> Result<LumpId, Error> {
    let success = FILESYSTEM
        .request(
            Request {
                target: path.to_string(),
                kind: RequestKind::Get,
            },
            &[],
        )
        .0?;
    match success {
        Success::Get(lump) => Ok(lump),
        _ => panic!("expected Success::Get, got {:?}", success),
    }
}

/// Read the bytes of a file into a `Vec<u8>`.
pub fn read_file(path: &str) -> Result<Vec<u8>, Error> {
    let lump = get_file(path)?;
    let lump = Lump::load_by_id(&lump);
    Ok(lump.get_data())
}

/// List all files and directories inside of a path.
pub fn list_files(path: &str) -> Result<Vec<FileInfo>, Error> {
    let success = FILESYSTEM
        .request(
            Request {
                target: path.to_string(),
                kind: RequestKind::List,
            },
            &[],
        )
        .0?;
    match success {
        Success::List(files) => Ok(files),
        _ => panic!("expected Success::List, got {:?}", success),
    }
}
