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

use std::path::PathBuf;

use hearth_core::{
    async_trait, cargo_process_metadata, hearth_types::fs::*, process::ProcessMetadata, utils::*,
};

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
        let target = match PathBuf::try_from(&request.data.target) {
            Ok(target) => target,
            Err(_) => return Error::InvalidTarget.into(),
        };

        let mut path = self.root.to_path_buf();
        for component in target.components() {
            match component {
                std::path::Component::Normal(normal) => path.push(normal),
                _ => return Error::DirectoryTraversal.into(),
            }
        }

        let success = match request.data.kind {
            RequestKind::Get => {
                let contents = match std::fs::read(path) {
                    Ok(contents) => contents,
                    Err(_) => todo!(),
                };

                let lump = request.runtime.lump_store.add_lump(contents.into()).await;

                Success::Get(lump)
            }
            RequestKind::List => {
                let dirs = match std::fs::read_dir(path) {
                    Ok(dirs) => dirs,
                    Err(_) => todo!(),
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

                Success::List(dirs)
            }
        };

        ResponseInfo {
            data: Ok(success),
            caps: vec![],
        }
    }
}

impl ServiceRunner for FsPlugin {
    const NAME: &'static str = "hearth.fs.Filesystem";

    fn get_process_metadata() -> ProcessMetadata {
        let mut meta = cargo_process_metadata!();
        meta.description =
            Some("The native filesystem access service. Accepts FsRequest.".to_string());
        meta
    }
}

impl FsPlugin {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}
