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

use std::marker::PhantomData;

use crate::component::{ComponentId, Layout};
use crate::{EntityId, World};

use bytemuck::{AnyBitPattern, Pod};
use fortuples::fortuples;

#[derive(Default)]
pub struct QueryParameters {
    pub view: Layout,
}

impl QueryParameters {
    pub fn include<T: 'static>(&mut self) {
        self.view.insert::<T>();
    }

    pub fn include_cid(&mut self, cid: ComponentId) {
        self.view.insert_cid(cid);
    }
}

pub struct Query {
    params: QueryParameters,
}

impl Query {
    pub fn new(params: QueryParameters) -> Self {
        Self { params }
    }

    pub fn evaluate<'a>(&self, w: &'a mut World) -> QueryResult<'a> {
        let mut entities = Vec::new();

        for e in w.entities.keys() {
            let layout = w.get_layout(*e).unwrap();
            if self.params.view.is_subset(&layout) {
                entities.push(*e);
            }
        }

        QueryResult { entities, world: w }
    }
}

pub struct StaticQuery<T> {
    inner: Query,
    _view: PhantomData<T>,
}

impl<T: IntoQuery> StaticQuery<T> {
    pub fn iter_mut<'a>(&self, w: &'a mut World) -> QueryViewIter<'a, T> {
        let result = self.inner.evaluate(w);

        QueryViewIter {
            result,
            idx: 0,
            _view: Default::default(),
        }
    }
}

pub struct QueryViewIter<'a, T> {
    result: QueryResult<'a>,
    idx: usize,
    _view: PhantomData<T>,
}

impl<'a, T: Fetch> Iterator for QueryViewIter<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let e = self.result.entities.get(self.idx)?;
        self.idx += 1;
        Some(T::fetch(*e, self.result.world))
    }
}

pub struct QueryResult<'a> {
    entities: Vec<EntityId>,
    world: &'a mut World,
}

impl<'a> QueryResult<'a> {
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn write_entities(&self, dst: &mut [EntityId]) {
        assert_eq!(
            dst.len(),
            self.len(),
            "Destination for QueryResult::write_entities() is missized."
        );

        dst.copy_from_slice(self.entities.as_slice());
    }

    pub fn write_components<T: AnyBitPattern>(&self, dst: &mut [T]) {
        assert_eq!(
            dst.len(),
            self.len(),
            "Destination for QueryResult::write_components<{}>() is missized.",
            std::any::type_name::<T>()
        );

        for (e, dst) in self.entities.iter().zip(dst.iter_mut()) {
            // TODO better panic message on unwrap
            *dst = *self.world.get::<T>(*e).unwrap();
        }
    }

    pub fn get_entities(&self) -> Vec<EntityId> {
        let mut dst = Vec::new();
        dst.resize(self.len(), EntityId::default());
        self.write_entities(&mut dst);
        dst
    }

    pub fn get_components<T: AnyBitPattern>(&self) -> Vec<T> {
        let len = self.len();
        let mut dst = Vec::with_capacity(len);
        unsafe { dst.set_len(len) };
        self.write_components(&mut dst);
        dst
    }
}

pub trait IntoQuery: Sized {
    fn query() -> StaticQuery<Self>;
}

impl<T: Fetch> IntoQuery for T {
    fn query() -> StaticQuery<Self> {
        let mut params = Default::default();
        T::build_query(&mut params);

        StaticQuery {
            inner: Query::new(params),
            _view: Default::default(),
        }
    }
}

pub trait Fetch: Sized {
    // TODO this is only temporary; it doesn't create any generic/efficient iterators.
    fn fetch(e: EntityId, w: &mut World) -> Self;
    fn build_query(query: &mut QueryParameters);
}

impl<'a, T: Pod + 'static> Fetch for &'a T {
    fn fetch(e: EntityId, w: &mut World) -> Self {
        unsafe { std::mem::transmute(w.get::<T>(e).unwrap()) }
    }

    fn build_query(query: &mut QueryParameters) {
        query.include::<T>();
    }
}

impl<'a, T: Pod + 'static> Fetch for &'a mut T {
    fn fetch(e: EntityId, w: &mut World) -> Self {
        unsafe { std::mem::transmute(w.get_mut::<T>(e).unwrap()) }
    }

    fn build_query(query: &mut QueryParameters) {
        query.include::<T>();
    }
}

impl<'a> Fetch for EntityId {
    fn fetch(e: EntityId, _w: &mut World) -> Self {
        e
    }

    fn build_query(_query: &mut QueryParameters) {
        // fetching entities in a query will not modify the parameters
    }
}

fortuples! {
    #[tuples::min_size(1)]
    #[tuples::max_size(8)]
    impl Fetch for #Tuple
    where
        #(#Member: Fetch),*
    {
        fn fetch(e: EntityId, w: &mut World) -> Self {
            ( #(#Member::fetch(e, w)),* )
        }

        fn build_query(query: &mut QueryParameters) {
            #(#Member::build_query(query));*
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::component::ComponentId;

    #[test]
    fn query_single() {
        let mut w = World::new();
        let val = 42u8;
        let e = w.spawn();
        w.insert(e, val);
        let cid = ComponentId::from_val_type(&val);

        let query_params = QueryParameters {
            view: Layout::from(vec![cid]),
        };

        let query = Query::new(query_params);
        let result = query.evaluate(&mut w);

        assert_eq!(result.get_entities(), vec![e]);
        assert_eq!(result.get_components::<u8>(), vec![val]);
    }

    #[test]
    fn query_iter() {
        let mut w = World::new();
        let e = w.spawn();
        let val1 = 360u32;
        let val2 = 42u8;
        w.insert(e, val1);
        w.insert(e, val2);

        let query = <(EntityId, &mut u32, &mut u8)>::query();
        for (query_e, c1, c2) in query.iter_mut(&mut w) {
            assert_eq!(e, query_e);
            assert_eq!(*c1, val1);
            assert_eq!(*c2, val2);

            *c1 += *c2 as u32;
        }

        assert_eq!(w.get::<u32>(e).copied(), Some(val1 + val2 as u32));
    }

    #[test]
    fn query_unmatched() {
        let mut w = World::new();
        let e = w.spawn();
        let c = 360u32;
        w.insert(e, c);

        let query = <(EntityId, &mut u8)>::query();
        for _ in query.iter_mut(&mut w) {
            panic!("Entities unexpectedly matched query.");
        }
    }
}
