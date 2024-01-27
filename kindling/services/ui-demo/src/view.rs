// Copyright (c) 2023 Marceline Cramer
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

//! Xilem-inspired application UI logic driven by diffing view trees.

use std::{
    any::Any,
    marker::PhantomData,
    sync::{atomic::AtomicUsize, Arc},
};

use kindling_host::prelude::*;

use crate::{FlowDirection, Widget};

/// A unique identifier for a specific persistent widget instance.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Id(usize);

impl Id {
    /// Generates a unique view ID.
    pub fn next() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(id)
    }
}

/// The location of a widget in a view tree by [Id].
///
/// Although each [Id] is unique, the path provides the means to locate the
/// associated view by walking down the tree to that view.
pub type IdPath<'a> = &'a [Id];

/// A generic type for any event that can be received by a view.
///
/// TODO: within the context of Hearth we'll probably want this to be a CBOR
/// or JSON value or something.
pub type Event = Box<dyn Any>;

/// The core UI logic abstraction for the given application data type.
///
/// Views can contain other views to form a "view tree". At every step in an
/// application's logic, the application creates a new view tree using the most
/// up-to-date version of the application's data.
///
/// View trees are ephemeral, and only last long enough to be diffed against
/// the previous view tree and to handle incoming widget events to that mutate
/// the application data for the next cycle.
pub trait View<T> {
    /// The type of the persistent view state to preserve between application
    /// cycles.
    ///
    /// This is a view-only piece of data that's used by views to diff against
    /// previous versions of views.
    type State;

    /// The persistent widget type that this view instantiates.
    type Widget: Widget;

    /// Initializes a [Self::State] and corresponding [Id] from an initial
    /// view value.
    ///
    /// The [Id] is returned by this function manually instead of being
    /// automatically decided by the parent in order to support views that
    /// have no corresponding widget and perform ID path passthrough of their
    /// children.
    ///
    /// The current [IdPath] leading to this view is also provided so that the
    /// view may instantiate its state with the full location of the view.
    fn build(&self, path: IdPath<'_>) -> (Id, Self::State, Self::Widget);

    /// Diffs a previous view against the current one in order to perform
    /// mutations to the view's persistent state and widget.
    ///
    /// TODO: why does Xilem make the [Id] reference mutable? when would you
    /// want to discard a widget and construct a new one instead of just
    /// modifying it? is that a DOM accommodation, or something more
    /// fundamental?
    ///
    /// TODO: what's the purpose of `ChangeFlags` in Xilem? shouldn't
    /// propagating sets of widget mutations be the widget tree's job?
    fn rebuild(&self, id: &Id, state: &mut Self::State, widget: &mut Self::Widget, prev: &Self);

    /// Handles an event targeting this view or one of its children and
    /// potentially mutates the app state.
    ///
    /// TODO: do we need to have an equivalent of Xilem's `MessageResult`?
    /// We don't need actions since we have a dedicated widget tree, we
    /// shouldn't need to manually rebuild, no-op is a given for no return
    /// type, and as far as I can tell we shouldn't need a special return type
    /// for when an ID is stale.
    fn event(
        &self,
        path: IdPath<'_>,
        state: &mut Self::State,
        widget: &mut Self::Widget,
        ev: Event,
        app: &mut T,
    );
}

pub struct Label(pub Arc<bdf::Font>, pub String);

impl<T> View<T> for Label {
    type State = ();
    type Widget = crate::Label;

    fn build(&self, _path: IdPath<'_>) -> (Id, Self::State, Self::Widget) {
        let widget = crate::Label::new(self.0.to_owned(), self.1.to_owned());
        (Id::next(), (), widget)
    }

    fn rebuild(&self, _id: &Id, _state: &mut Self::State, widget: &mut Self::Widget, prev: &Self) {
        if !Arc::ptr_eq(&self.0, &prev.0) || self.1 != prev.1 {
            *widget = crate::Label::new(self.0.to_owned(), self.1.to_owned());
        }
    }

    fn event(
        &self,
        _path: IdPath<'_>,
        _state: &mut Self::State,
        _widget: &mut Self::Widget,
        _ev: Event,
        _app: &mut T,
    ) {
    }
}

pub struct Button<F>(pub F);

impl<T, F: Fn(&mut T)> View<T> for Button<F> {
    type State = ();

    type Widget = crate::Button;

    fn build(&self, path: IdPath<'_>) -> (Id, Self::State, Self::Widget) {
        let id = Id::next();
        let mut full_path = path.to_vec();
        full_path.push(id);
        let button = crate::Button::new(full_path);
        (id, (), button)
    }

    fn rebuild(
        &self,
        _id: &Id,
        _state: &mut Self::State,
        _widget: &mut Self::Widget,
        _prev: &Self,
    ) {
    }

    fn event(
        &self,
        _path: IdPath<'_>,
        _state: &mut Self::State,
        _widget: &mut Self::Widget,
        _ev: Event,
        app: &mut T,
    ) {
        (self.0)(app);
    }
}

/// A slider view (not widget).
pub struct Slider<F>(pub F);

impl<T, F: Fn(&mut T, i32)> View<T> for Slider<F> {
    type State = ();
    type Widget = crate::Slider;

    fn build(&self, path: IdPath<'_>) -> (Id, Self::State, Self::Widget) {
        let id = Id::next();
        let mut full_path = path.to_vec();
        full_path.push(id);
        let slider = crate::Slider::new(full_path);
        (id, (), slider)
    }

    fn rebuild(
        &self,
        _id: &Id,
        _state: &mut Self::State,
        _widget: &mut Self::Widget,
        _prev: &Self,
    ) {
        // nothing to do because we're already going to receive the change event
    }

    fn event(
        &self,
        _path: IdPath<'_>,
        _state: &mut Self::State,
        _widget: &mut Self::Widget,
        ev: Event,
        app: &mut T,
    ) {
        let pos: Box<i32> = ev.downcast().unwrap();
        (self.0)(app, *pos);
    }
}

pub struct Flow<T, A, B> {
    children: (A, B),
    dir: FlowDirection,
    _app: PhantomData<T>,
}

impl<T, A: View<T>, B: View<T>> View<T> for Flow<T, A, B> {
    type State = ((Id, A::State), (Id, B::State));
    type Widget = crate::Flow;

    fn build(&self, path: IdPath<'_>) -> (Id, Self::State, Self::Widget) {
        let id = Id::next();
        let mut child_path = path.to_vec();
        child_path.push(id);
        let (a_id, a_state, a_widget) = self.children.0.build(&child_path);
        let (b_id, b_state, b_widget) = self.children.1.build(&child_path);

        let flow = crate::Flow::new(self.dir).child(a_widget).child(b_widget);

        (id, ((a_id, a_state), (b_id, b_state)), flow)
    }

    fn rebuild(&self, _id: &Id, state: &mut Self::State, widget: &mut Self::Widget, prev: &Self) {
        self.children.0.rebuild(
            &state.0 .0,
            &mut state.0 .1,
            widget.children[0].inner.as_any().downcast_mut().unwrap(),
            &prev.children.0,
        );

        self.children.1.rebuild(
            &state.1 .0,
            &mut state.1 .1,
            widget.children[1].inner.as_any().downcast_mut().unwrap(),
            &prev.children.1,
        );
    }

    fn event(
        &self,
        path: IdPath<'_>,
        state: &mut Self::State,
        widget: &mut Self::Widget,
        ev: Event,
        app: &mut T,
    ) {
        // index 0 is the ID of the parent's (this) view, so we skip it
        let child_id = path[1];
        let remainder = &path[1..];

        // fetch dynamic widget reference based on selected child
        let child = if child_id == state.0 .0 {
            &mut widget.children[0].inner
        } else if child_id == state.1 .0 {
            &mut widget.children[1].inner
        } else {
            // child ID not found, so this event is out-of-date or invalid.
            warn!("ID out of date: {:?}", path);
            return;
        };

        if child_id == state.0 .0 {
            self.children.0.event(
                remainder,
                &mut state.0 .1,
                child.as_any().downcast_mut().unwrap(),
                ev,
                app,
            );
        } else {
            self.children.1.event(
                remainder,
                &mut state.1 .1,
                child.as_any().downcast_mut().unwrap(),
                ev,
                app,
            );
        }
    }
}

impl<T, A, B> Flow<T, A, B>
where
    A: View<T>,
    B: View<T>,
{
    pub fn new(dir: FlowDirection, a: A, b: B) -> Self {
        Self {
            children: (a, b),
            dir,
            _app: PhantomData,
        }
    }

    pub fn row(a: A, b: B) -> Self {
        Self::new(FlowDirection::Horizontal, a, b)
    }

    pub fn column(a: A, b: B) -> Self {
        Self::new(FlowDirection::Vertical, a, b)
    }
}
