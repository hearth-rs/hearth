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
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};

use tracing::{field::Visit, span, Subscriber};

/// Subscribes to tracing events and formats them through the API to the host
pub struct ProcessSubscriber {
    next_span_id: AtomicUsize,
}

impl ProcessSubscriber {
    pub fn new() -> Self {
        Self {
            next_span_id: AtomicUsize::new(1),
        }
    }
}

impl Subscriber for ProcessSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        let id = self.next_span_id.fetch_add(1, Ordering::SeqCst);

        span::Id::from_u64(id as u64)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut message = String::new();

        let mut visitor = FmtEvent {
            message: &mut message,
            needs_comma: false,
        };

        event.record(&mut visitor);

        let module = event.metadata().target();
        let level = (*event.metadata().level()).into();

        // TODO: Support structured logging
        crate::log(level, module, &message);
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}

pub struct FmtEvent<'a> {
    pub message: &'a mut String,
    pub needs_comma: bool,
}

impl<'a> Visit for FmtEvent<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let comma = if self.needs_comma { ", " } else { "" };
        match field.name() {
            "message" => {
                write!(self.message, "{comma}{value:?}").unwrap();
                self.needs_comma = true;
            }
            name => {
                write!(self.message, "{comma}{name}={value:?}").unwrap();
            }
        }
    }
}
