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

use std::any::TypeId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::Pod;
use fortuples::fortuples;

/// An identifier for a component.
///
/// Component IDs can be generated at either runtime through custom IDs or
/// at compile-time with Rust types. This fusion of runtime and compile-time
/// component logic allows Dominion to provide both scriptable runtime logic
/// and idiomatic usage from Rust.
///
/// The upper bit of the ID is set if the ID has been generated from a Rust
/// type, and is unset if it's a custom runtime ID.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct ComponentId(u64);

impl ComponentId {
    const IS_TYPE_MASK: u64 = !(u64::MAX >> 1);

    pub fn is_type(&self) -> bool {
        (self.0 & Self::IS_TYPE_MASK) != 0
    }

    pub fn from_type<T: 'static>() -> Self {
        let id = TypeId::of::<T>();
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);

        let hash = hasher.finish();
        Self(hash | Self::IS_TYPE_MASK)
    }

    pub fn from_val_type<T: 'static>(_val: &T) -> Self {
        Self::from_type::<T>()
    }

    pub fn from_custom(id: u64) -> Self {
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(id);
        let hash = hasher.finish();
        Self(hash & !Self::IS_TYPE_MASK)
    }
}

/// Information about a component.
///
/// `size` and `alignment` are used for determining the memory of the layout
/// of the component, and `name` is an optional debug label for this component. 
#[derive(Clone, Debug)]
pub struct ComponentInfo {
    pub name: Option<String>,
    pub size: usize,
    pub alignment: usize,
}

impl ComponentInfo {
    pub fn from_type<T: 'static>() -> Self {
        Self {
            name: Some(std::any::type_name::<T>().to_string()),
            size: std::mem::size_of::<T>(),
            alignment: std::mem::align_of::<T>(),
        }
    }
}

#[derive(Clone)]
pub struct ComponentData<'a> {
    pub id: ComponentId,
    pub data: &'a [u8],
}

pub trait AsComponentData {
    fn as_component_data(&self) -> ComponentData<'_>;
}

impl<'a> AsComponentData for ComponentData<'a> {
    fn as_component_data(&self) -> ComponentData<'a> {
        Self {
            id: self.id,
            data: self.data,
        }
    }
}

impl<'a, T: Pod> AsComponentData for T {
    fn as_component_data(&self) -> ComponentData<'_> {
        ComponentData {
            id: ComponentId::from_type::<T>(),
            data: bytemuck::bytes_of(self),
        }
    }
}

pub trait AsMultiComponentData {
    fn len(&self) -> usize;
    fn get_data<'a>(&'a self, datas: &mut Vec<ComponentData<'a>>);
}

fortuples! {
    #[tuples::max_size(8)]
    impl AsMultiComponentData for #Tuple
    where
        #(#Member: AsComponentData),*
    {
        fn len(&self) -> usize {
            #len(Tuple)
        }

        fn get_data<'a>(&'a self, datas: &mut Vec<ComponentData<'a>>) {
            datas.extend_from_slice(&[
                #(#self.as_component_data()),*
            ]);
        }
    }
}

/// A list of components.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Layout(Vec<ComponentId>);

impl From<Vec<ComponentId>> for Layout {
    fn from(mut src: Vec<ComponentId>) -> Self {
        src.sort_unstable_by_key(|cid| cid.0);
        Self(src)
    }
}

impl Layout {
    pub fn is_subset(&self, parent: &Layout) -> bool {
        // TODO this can be improved by iterating both layouts at once.
        self.0.iter().all(|cid| parent.0.contains(cid))
    }

    pub fn insert_cid(&mut self, cid: ComponentId) {
        if !self.0.contains(&cid) {
            self.0.push(cid);
        }

        // TODO please insert without doing a full sort afterwards...
        self.0.sort_by_key(|cid| cid.0);
    }

    pub fn insert<T: 'static>(&mut self) {
        self.insert_cid(ComponentId::from_type::<T>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_from_type() {
        let f32_id = ComponentId::from_type::<f32>();
        let u32_id = ComponentId::from_type::<u32>();
        assert!(f32_id.is_type());
        assert!(u32_id.is_type());
        assert_ne!(f32_id, u32_id);
    }

    #[test]
    fn layout_eq() {
        let cid1 = ComponentId::from_custom(0);
        let cid2 = ComponentId::from_custom(1);
        let cid3 = ComponentId::from_custom(2);

        let l1 = Layout::from(vec![cid1, cid2]);
        let l2 = Layout::from(vec![cid2, cid1]);
        let l3 = Layout::from(vec![cid1, cid2, cid3]);
        let l4 = Layout::from(vec![cid3, cid2, cid1]);

        assert_eq!(l1, l2);
        assert_eq!(l3, l4);
        assert_ne!(l1, l3);
        assert_ne!(l2, l4);
    }

    #[test]
    fn layout_subset() {
        let cid1 = ComponentId::from_custom(0);
        let cid2 = ComponentId::from_custom(1);
        let cid3 = ComponentId::from_custom(2);

        let l1 = Layout::from(vec![cid1, cid2]);
        let l2 = Layout::from(vec![cid1, cid2, cid3]);

        assert!(l1.is_subset(&l2));
        assert!(!l2.is_subset(&l1));
    }
}
