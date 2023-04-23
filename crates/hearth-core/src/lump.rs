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

use bytes::{Buf, Bytes};
use hearth_rpc::*;
use hearth_types::*;
use remoc::robj::lazy_blob::LazyBlob;
use remoc::rtc::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, error};

pub use bytes;

#[derive(Debug)]
struct Lump {
    data: Bytes,
    blob: LazyBlob,
}

#[derive(Debug, Default)]
pub struct LumpStoreImpl {
    store: RwLock<HashMap<LumpId, Lump>>,
}

#[async_trait]
impl LumpStore for LumpStoreImpl {
    async fn upload_lump(&self, id: Option<LumpId>, data: LazyBlob) -> ResourceResult<LumpId> {
        if let Some(id) = id {
            debug!("Beginning lump upload (known ID: {})", id);
        } else {
            debug!("Beginning lump upload (unknown ID)");
        }

        if let Some(id) = id {
            if self.store.read().await.contains_key(&id) {
                debug!("Already have lump {}", id);
                return Ok(id);
            }
        }

        let data = match data.get().await {
            Ok(data) => data,
            Err(err) => {
                eprintln!("Downloading lump failed: {:?}", err);
                return Err(ResourceError::Unavailable);
            }
        };

        let checked_id = LumpId(
            blake3::Hasher::new()
                .update(data.chunk())
                .finalize()
                .as_bytes()
                .to_owned(),
        );

        if let Some(expected_id) = id {
            if expected_id != checked_id {
                error!(
                    "Lump hash mismatch (expected {}, got {})",
                    expected_id, checked_id
                );

                return Err(ResourceError::BadParams);
            }
        }

        debug!("Storing lump {}", checked_id);
        let data = Bytes::from(data);
        let blob = LazyBlob::new(data.clone());
        let lump = Lump { data, blob };
        let mut store = self.store.write().await;
        store.insert(checked_id, lump);
        Ok(checked_id)
    }

    async fn download_lump(&self, id: LumpId) -> ResourceResult<LazyBlob> {
        debug!("Downloading lump {}", id);
        self.store
            .read()
            .await
            .get(&id)
            .ok_or(ResourceError::Unavailable)
            .map(|l| l.blob.to_owned())
    }
}

impl LumpStoreImpl {
    pub fn new() -> Self {
        Self {
            store: Default::default(),
        }
    }

    pub async fn add_lump(&self, data: Bytes) -> LumpId {
        let id = LumpId(
            blake3::Hasher::new()
                .update(data.chunk())
                .finalize()
                .as_bytes()
                .to_owned(),
        );

        self.store.write().await.entry(id).or_insert_with(|| {
            debug!("Storing lump {}", id);
            let blob = LazyBlob::new(data.clone());
            Lump { data, blob }
        });

        id
    }

    pub async fn get_lump(&self, id: &LumpId) -> Option<Bytes> {
        self.store
            .read()
            .await
            .get(id)
            .map(|lump| lump.data.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hearth_rpc::remoc::rtc::ServerShared;

    use super::*;

    fn make_id(bytes: &[u8]) -> LumpId {
        LumpId(
            blake3::Hasher::new()
                .update(bytes)
                .finalize()
                .as_bytes()
                .to_owned(),
        )
    }

    /// LazyBlobs don't work locally, so this needs to be pulled off in order
    /// to get them to work in these tests.
    async fn spawn_store() -> LumpStoreClient {
        use remoc::rch::base::{Receiver, Sender};

        let (a, b) = tokio::io::duplex(4096);
        let (a_rx, a_tx) = tokio::io::split(a);
        let (b_rx, b_tx) = tokio::io::split(b);

        let join_a = tokio::spawn(async move {
            let (conn, tx, _rx): (_, Sender<LumpStoreClient>, Receiver<()>) =
                remoc::Connect::io(Default::default(), a_rx, a_tx)
                    .await
                    .unwrap();
            tokio::task::spawn(conn);
            tx
        });

        let join_b = tokio::spawn(async move {
            let (conn, _tx, rx): (_, Sender<()>, Receiver<LumpStoreClient>) =
                remoc::Connect::io(Default::default(), b_rx, b_tx)
                    .await
                    .unwrap();
            tokio::task::spawn(conn);
            rx
        });

        let store = LumpStoreImpl::new();
        let store = Arc::new(store);
        let (store_server, store) = LumpStoreServerShared::new(store, 1024);

        tokio::spawn(async move {
            store_server.serve(true).await;
        });

        let mut tx = join_a.await.unwrap();
        let mut rx = join_b.await.unwrap();

        tx.send(store).await.unwrap();
        rx.recv().await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn create_store() {
        let _store = spawn_store();
    }

    #[tokio::test]
    async fn upload_then_download() {
        const DATA: &[u8] = b"Hello, world!";
        let id = make_id(DATA);
        let data_blob = LazyBlob::new(DATA.into());
        let store = spawn_store().await;

        let uploaded = store
            .upload_lump(Some(id), data_blob)
            .await
            .expect("Failed to upload");

        assert_eq!(uploaded, id);

        let downloaded = store
            .download_lump(id)
            .await
            .expect("Failed to download")
            .get()
            .await
            .unwrap();

        assert_eq!(downloaded.chunk(), DATA);
    }

    #[tokio::test]
    async fn wrong_id() {
        const DATA: &[u8] = b"Hello, world!";
        let wrong = make_id(b"Wrong data!");
        let data_blob = LazyBlob::new(DATA.into());
        let store = spawn_store().await;
        let result = store.upload_lump(Some(wrong), data_blob).await;
        match result {
            Err(ResourceError::BadParams) => {}
            result => panic!("Unexpected result: {:?}", result),
        }
    }
}
