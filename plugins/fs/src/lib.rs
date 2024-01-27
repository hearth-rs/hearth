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

use std::{
    fs::{read, read_dir},
    path::{Component, PathBuf},
};

use hearth_runtime::{
    async_trait, hearth_macros::GetProcessMetadata, hearth_schema::fs::*, utils::*,
};

/// The native filesystem access service. Accepts FsRequest.
#[derive(GetProcessMetadata)]
pub struct FsPlugin {
    root: PathBuf,
}

#[async_trait]
impl RequestResponseProcess for FsPlugin {
    type Request = Request;
    type Response = Response;

    async fn on_request<'a>(
        &'a mut self,
        request: &mut RequestInfo<'a, Request>,
    ) -> ResponseInfo<'a, Response> {
        ResponseInfo {
            data: self.handle_request(request).await,
            caps: vec![],
        }
    }
}

impl ServiceRunner for FsPlugin {
    const NAME: &'static str = "hearth.fs.Filesystem";
}

impl FsPlugin {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    async fn handle_request<'a>(&'a mut self, request: &mut RequestInfo<'a, Request>) -> Response {
        let target = PathBuf::try_from(&request.data.target).map_err(|_| Error::InvalidTarget)?;

        let mut path = self.root.to_path_buf();
        for component in target.components() {
            match component {
                Component::Normal(normal) => path.push(normal),
                _ => return Err(Error::DirectoryTraversal),
            }
        }

        let to_response_error = |err: std::io::Error| -> Error {
            use std::io::ErrorKind::*;
            match err.kind() {
                NotFound => Error::NotFound,
                PermissionDenied => Error::PermissionDenied,
                e => Error::Other(e.to_string()),
            }
        };

        match request.data.kind {
            RequestKind::Get => {
                let contents = match read(path) {
                    Ok(contents) => contents,
                    Err(e) => return Err(to_response_error(e)),
                };

                let lump = request.runtime.lump_store.add_lump(contents.into()).await;

                Ok(Success::Get(lump))
            }
            RequestKind::List => {
                let dirs = match read_dir(path) {
                    Ok(dirs) => dirs,
                    Err(e) => return Err(to_response_error(e)),
                };

                let dirs: Vec<_> = dirs
                    .into_iter()
                    .map(|dir| {
                        let dir = dir.unwrap();

                        FileInfo {
                            name: dir.file_name().to_string_lossy().to_string(),
                        }
                    })
                    .collect();

                Ok(Success::List(dirs))
            }
        }
    }
}
