// Copyright (c) 2023 the Hearth contributors.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::HashMap;

use bytemuck::{AnyBitPattern, Pod};

pub mod component;
pub mod query;

use component::*;

pub type EntityId = u64;

pub struct World {
    entities: HashMap<EntityId, HashMap<ComponentId, Vec<u8>>>,
    components: HashMap<ComponentId, ComponentInfo>,
    next_id: EntityId,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            components: Default::default(),
            next_id: 0,
        }
    }

    /// Allocates an empty entity.
    pub fn spawn(&mut self) -> EntityId {
        let id = self.next_id;
        self.entities.insert(id, Default::default());
        self.next_id += 1;
        id
    }

    /// Destroys an entity.
    pub fn destroy(&mut self, e: EntityId) {
        self.entities.remove(&e);
    }

    /// Inserts a component into an entity.
    pub fn insert<T: AsComponentData>(&mut self, e: EntityId, c: T) {
        if let Some(e) = self.entities.get_mut(&e) {
            let data = c.as_component_data();
            let cid = data.id;
            let storage = data.data.to_vec();
            e.insert(cid, storage);
        }
    }

    /// Removes a component from an entity.
    pub fn remove<T: Pod>(&mut self, e: EntityId) {
        if let Some(e) = self.entities.get_mut(&e) {
            let cid = ComponentId::from_type::<T>();
            e.remove(&cid);
        }
    }

    /// Looks up a component on an entity.
    pub fn get<T: AnyBitPattern>(&self, e: EntityId) -> Option<&T> {
        let e = self.entities.get(&e)?;
        let cid = ComponentId::from_type::<T>();
        let c = e.get(&cid)?;
        Some(bytemuck::from_bytes(c))
    }

    /// Mutably looks up a component on an entity.
    pub fn get_mut<T: Pod>(&mut self, e: EntityId) -> Option<&mut T> {
        let e = self.entities.get_mut(&e)?;
        let cid = ComponentId::from_type::<T>();
        let c = e.get_mut(&cid)?;
        Some(bytemuck::from_bytes_mut(c))
    }

    /// Gets the layout of an entity.
    ///
    /// Returns [None] if the entity does not exist.
    pub fn get_layout(&self, e: EntityId) -> Option<Layout> {
        let e = self.entities.get(&e)?;
        Some(e.keys().copied().collect::<Vec<ComponentId>>().into())
    }

    /// Creates a new entity with the given components.
    pub fn push<T: AsMultiComponentData>(&mut self, components: T) -> EntityId {
        let mut datas = Vec::with_capacity(components.len());
        let e = self.spawn();
        components.get_data(&mut datas);

        for data in datas {
            self.insert(e, data);
        }

        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_world() {
        World::new();
    }

    #[test]
    fn spawn() {
        let mut w = World::new();
        let e1 = w.spawn();
        let e2 = w.spawn();
        assert_ne!(e1, e2);
    }

    #[test]
    fn insert_get() {
        let mut w = World::new();
        let e = w.spawn();
        let c = 42u8;
        w.insert(e, c);
        assert_eq!(w.get(e), Some(&c));
    }

    #[test]
    fn insert_get_mut() {
        let mut w = World::new();
        let e = w.spawn();
        let c = 42u8;
        let d = 36u8;
        let result = c + d;
        w.insert(e, c);
        *w.get_mut::<u8>(e).unwrap() += d;
        assert_eq!(w.get(e), Some(&result));
    }

    #[test]
    fn insert_remove() {
        let mut w = World::new();
        let e = w.spawn();
        let c = 42u8;
        w.insert(e, c);
        w.remove::<u8>(e);
        assert_eq!(w.get::<u8>(e), None);
    }

    #[test]
    fn insert_layout() {
        let mut w = World::new();
        let e = w.spawn();
        let c = 42u8;
        let cid = ComponentId::from_val_type(&c);
        w.insert(e, c);
        assert_eq!(w.get_layout(e), Some(vec![cid].into()));
    }

    #[test]
    fn push_single_get() {
        let mut w = World::new();
        let val = 42u8;
        let e = w.push((val,));
        assert_eq!(w.get(e), Some(&val));
    }

    #[test]
    fn push_tuple_get() {
        let mut w = World::new();
        let val1 = 42u8;
        let val2 = 320u32;
        let e = w.push((val1, val2));
        assert_eq!(w.get(e), Some(&val1));
        assert_eq!(w.get(e), Some(&val2));
    }

    #[test]
    fn push_tuple_layout() {
        let mut w = World::new();
        let val1 = 42u8;
        let val2 = 320u32;
        let e = w.push((val1, val2));
        let cid1 = ComponentId::from_val_type(&val1);
        let cid2 = ComponentId::from_val_type(&val2);
        assert_eq!(w.get_layout(e), Some(vec![cid1, cid2].into()));
    }
}
