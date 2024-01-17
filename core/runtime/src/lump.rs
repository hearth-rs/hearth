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
use hearth_schema::*;
use tokio::sync::RwLock;
use tracing::debug;

pub use bytes;

#[derive(Debug)]
struct Lump {
    data: Bytes,
}

#[derive(Debug, Default)]
pub struct LumpStoreImpl {
    store: RwLock<HashMap<LumpId, Lump>>,
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

        let mut store = self.store.write().await;
        store.entry(id).or_insert_with(|| {
            debug!("Storing lump {}", id);
            Lump { data }
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
