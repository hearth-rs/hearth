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

use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::lump::LumpStoreImpl;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use hearth_schema::LumpId;
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error};

#[async_trait]
pub trait AssetLoader: Send + Sync + 'static {
    type Asset: Send + Sync + 'static;

    async fn load_asset(&self, store: &AssetStore, data: &[u8]) -> Result<Self::Asset>;
}

/// Helper trait to implement [AssetLoader] for asset loaders that load from
/// JSON-encoded data.
#[async_trait]
pub trait JsonAssetLoader: Send + Sync + 'static {
    type Asset: Send + Sync + 'static;
    type Data: for<'a> Deserialize<'a> + Send;

    async fn load_asset(&self, store: &AssetStore, data: Self::Data) -> Result<Self::Asset>;
}

#[async_trait]
impl<T: JsonAssetLoader> AssetLoader for T {
    type Asset = T::Asset;

    async fn load_asset(&self, store: &AssetStore, data: &[u8]) -> Result<T::Asset> {
        let data: T::Data = serde_json::from_slice(data)
            .with_context(|| format!("Deserializing asset from {}", type_name::<T::Data>()))?;

        self.load_asset(store, data).await
    }
}

/// Loads and caches assets loaded from a loader.
pub struct AssetPool<T: AssetLoader> {
    loader: Mutex<T>,
    assets: RwLock<HashMap<LumpId, Arc<T::Asset>>>,
}

impl<T: AssetLoader> AssetPool<T> {
    pub fn new(loader: T) -> Self {
        Self {
            loader: Mutex::new(loader),
            assets: Default::default(),
        }
    }

    async fn load_asset(
        &self,
        store: &AssetStore,
        lump: &LumpId,
        data: &[u8],
    ) -> Result<Arc<T::Asset>> {
        let assets = self.assets.read().await;
        if let Some(asset) = assets.get(lump) {
            Ok(asset.to_owned())
        } else {
            // switch to write lock
            drop(assets);
            let mut assets = self.assets.write().await;

            let loader = self.loader.lock().await;
            let asset = loader.load_asset(store, data).await?;
            let asset = Arc::new(asset);
            assets.insert(*lump, asset.to_owned());
            Ok(asset)
        }
    }
}

pub struct AssetStore {
    pools: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    lump_store: Arc<LumpStoreImpl>,
}

impl AssetStore {
    pub fn new(lump_store: Arc<LumpStoreImpl>) -> Self {
        Self {
            pools: HashMap::new(),
            lump_store,
        }
    }

    pub fn add_loader<T: AssetLoader>(&mut self, loader: T) {
        let type_name = std::any::type_name::<T>();
        debug!("Adding asset loader {:?}", type_name);

        let type_id = TypeId::of::<T>();
        if self.pools.contains_key(&type_id) {
            error!("Asset loader {:?} has already been added!", type_name);
            return;
        }

        let pool = AssetPool::new(loader);
        self.pools.insert(type_id, Box::new(pool));
    }

    pub fn has_loader<T: AssetLoader>(&self) -> bool {
        self.pools.contains_key(&TypeId::of::<T>())
    }

    pub async fn load_asset<T: AssetLoader>(&self, lump: &LumpId) -> Result<Arc<T::Asset>> {
        let type_name = std::any::type_name::<T>();
        let type_id = TypeId::of::<T>();
        let pool = self
            .pools
            .get(&type_id)
            .ok_or_else(|| anyhow!("Could not find asset loader '{:?}", type_name))?;
        let pool: &AssetPool<T> = pool.downcast_ref().unwrap();
        let data = self
            .lump_store
            .get_lump(lump)
            .await
            .ok_or_else(|| anyhow!("Failed to get lump {}", lump))?;
        pool.load_asset(self, lump, &data).await
    }
}
