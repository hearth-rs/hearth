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

#![warn(missing_docs)]

use std::sync::{atomic::AtomicUsize, Arc};

use flue::{Mailbox, MailboxGroup, PostOffice, Table};
use hearth_schema::ProcessLogLevel;
use ouroboros::self_referencing;
use tracing::{debug, Span};

/// A local Hearth process. The main entrypoint for Hearth programming.
#[self_referencing]
pub struct Process {
    /// This process's [Table].
    pub table: Table,

    /// This process's [ProcessInfo].
    pub info: ProcessInfo,

    /// This process's [MailboxGroup].
    #[borrows(table)]
    #[covariant]
    pub group: MailboxGroup<'this>,

    /// A mailbox that receives signals from this process's parent.
    ///
    /// This field lasts the entire lifetime of a process and cannot be dropped.
    /// This is so that local processes can always be killed by their parents
    /// and can't go rogue.
    #[borrows(group)]
    #[covariant]
    pub parent: Mailbox<'this>,
}

/// The integer identifier for a local process.
///
/// Hidden from most guest-side code, but is used host-side for human-readable
/// process identifiers.
pub type ProcessId = usize;

/// Information about a running process with data distinguishing it from other processes.
pub struct ProcessInfo {
    /// The [ProcessId] of this process.
    pub pid: ProcessId,

    /// A tracing span for process logs.
    ///
    /// All tracing events originating from this span will be considered to be logs from this
    /// process
    pub process_span: Span,

    /// This process's [ProcessMetdata].
    pub meta: ProcessMetadata,
}

impl Drop for ProcessInfo {
    fn drop(&mut self) {
        debug!("despawning PID {}", self.pid);
    }
}

/// Static metadata about a process.
#[non_exhaustive]
#[derive(Clone, Debug, Default)]
pub struct ProcessMetadata {
    /// A short, human-readable identifier for this process's function.
    pub name: Option<String>,

    /// Longer documentation of this process's function.
    pub description: Option<String>,

    /// A list of authors of this process.
    pub authors: Option<Vec<String>>,

    /// A link to this process's source repository.
    pub repository: Option<String>,

    /// A link to the home page of this process.
    pub homepage: Option<String>,

    /// An SPDX license identifier of this process's software license.
    pub license: Option<String>,
}

/// A factory for making local instances of [Process].
pub struct ProcessFactory {
    post: Arc<PostOffice>,
    pid_gen: AtomicUsize,
}

impl ProcessFactory {
    /// Creates a new process factory in the given post office.
    pub fn new(post: Arc<PostOffice>) -> Self {
        Self {
            post,
            pid_gen: AtomicUsize::new(0),
        }
    }

    /// Spawns a process with an existing [Table].
    pub fn spawn_with_table(&self, meta: ProcessMetadata, table: Table) -> Process {
        // this results in guessable PIDs, but access to PIDs and operations
        // consuming PIDs is limited to the debugging infrastructure, which
        // should not be given to untrusted processes.
        let pid = self
            .pid_gen
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        debug!(%pid, ?meta, "spawning process");

        // Create a span for the process to log its events to.
        //
        // The events within this span will be handled as appropriate by the currently configured
        // subscriber, such as writing them to stderr or a file, or even over the network or in
        // the UI itself.
        let name = &meta.name;
        let process_span =
            tracing::debug_span!(parent: None, "process", label = name, process_id = pid);

        let id = ProcessInfo {
            pid,
            process_span,
            meta,
        };

        Process::new(
            table,
            id,
            |table| MailboxGroup::new(table),
            |store| store.create_mailbox().unwrap(),
        )
    }

    /// Spawns a process with a new table in this factory's [PostOffice].
    pub fn spawn(&self, meta: ProcessMetadata) -> Process {
        self.spawn_with_table(meta, Table::new(self.post.clone()))
    }
}

/// Log event emitted by a process.
#[derive(Clone, Debug, Hash)]
pub struct ProcessLogEvent {
    /// The level of this log event.
    pub level: ProcessLogLevel,

    /// Provides context to the event's location, such as a script module.
    pub module: String,

    /// The main message body of the log event.
    pub content: String,
    // TODO optional source code location?
    // TODO serializeable timestamp?
}
