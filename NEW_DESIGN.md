# Overview

Hearth is an always-on execution environment for building 3D virtual worlds
from the inside.

> TODO: bring the new tagline in here!

Hearth uses a **message-passing virtual machine** as the foundation for all of
its behavior. The VM supports **hot-reload** and **self-modification**: every
part of the Hearth environment can be updated and reloaded with new code
without shutting down the rest of the runtime.

The VM is also distributed across a **client-server network
architecture**: multiple clients may connect to a single server. Resources on
one peer can be made transparently accessible to any other peer.

Hearth combines its distributed VM with elements of your run-of-the-mill game
engine; features such as a 3D rendering engine, audio output, or desktop
windowing. However, unlike most other game engines and social VR platforms,
Hearth has no external editor. All Hearth content, including the tools used to
create Hearth content, is created inside of Hearth itself.

# Free Forever

- open source
- self-hosting
- AGPL licensing

# Inspiration

- Erlang and Elixir
- Smalltalk
- MUDs and MOOs such as LambdaMOO
- modern social VR platforms such as Neos, VRChat, or Resonite

# Host-Guest Boundary

Hearth isn't magic; it's a program that runs on your computer just like any
other program, and all programs exit eventually. We at Hearth refer to the
native Hearth program as **the host** or **the runtime**. The host implements
the Hearth VM, access to native resources such as audio and graphics, and most
importantly, the execution environment for **guests**.

> TODO: rephrase this ^^^ paragraph to target established audience knowledge level better

Hearth guests are **non-native extensions** to the Hearth runtime, i.e. scripts.
Guests can be freely loaded and unloaded by the host. However, guests do not
have access to native resources, so they depend on the host to expose those
resources to guests. The message protocols through which guests interact with
native resources from the host are defined by the `hearth-schema` crate.

The host has the unfortunate limitation that it **cannot be hot-reloaded**.
This is because the entirety of the host needs to be recompiled, so the
entirety of the host needs to be restarted to load any of the modified code.
One solution would be to break up the host into multiple dynamically-linked
libraries that can each be loaded and unloaded independently of each other.
However, that's a huge pain in the ass! Instead, Hearth adopts an ongoing goal
to **push as much Hearth code as possible guest-side**, since guests
are easily capable of hot-reload.

# Client-Server Architecture

Hearth has two binaries: `hearth-client` and `hearth-server`, which (clearly)
implement the Hearth client and server.

Clients and servers communicate over the Internet using [WebSocket]
(https://en.wikipedia.org/wiki/WebSocket), a standard web protocol for
bidirectional communication. The advantage of WebSocket over lower-level
transport mechanisms such as TCP is that WebSocket can interoperate with
reverse proxy software such as nginx, Caddy, or Apache. This allows users to
host and access Hearth over web URIs and to reach Hearth through firewalls that
block non-web internet traffic.

The underlying WebSocket transport layer is also encrypted using TLS (this is
known as WebSocket Secure). The Hearth server does not implement TLS encryption
on its own, since that functionality is best implemented by existing reverse
proxy software. However, the Hearth client does implement its own side of https://stackoverflow.com/questions/71409448/plantuml-in-latexTLS,
since no certificate management is necessary.

# WebAssembly

Hearth's first-class guest scripting language is
[WebAssembly](https://webassembly.org/), or "Wasm" for short.
WebAssembly is extremely performant, simple,
[a compile target for lots of languages](https://github.com/appcypher/awesome-wasm-langs),
and has especially good runtime support in Rust, Hearth's main development
language. Since Rust also supports Wasm as a compile target, Rust can be used
for both host- and guest-side development, reducing the development friction
of programming in both environments.

> TODO: discuss common use of Wasm and compare-contrast against our own usage
> TODO: emphasize the disconnect of "Web"Assembly from the web

WebAssembly is a sandboxed scripting environment that provides strong process
isolation. This is a cornerstone design point for Hearth's message-passing
architecture.

A major benefit of WebAssembly is that it has a very low-level data model. All
Wasm data lives inside a Wasm instance's memory, which is a linear array of
bytes. Hearth's interaction with Wasm largely deals with simple operations on
byte arrays, making the API surface layer between host and Wasm guest extremely
small.

Performance is another upside to WebAssembly, since an ongoing goal for Hearth
is to make as much code as possible run in the guest. Wasm has very little
performance overhead when compared to equivalent native code on the fastest
runtimes (which we use), which effectively eliminates sheer performance as a
concern in the quest to hot-reload *everything*.

# Lumps

- bulk data format
- content addressed
- more efficient than message-passing
- input to various host services

# Message-Passing VM

In Hearth, **everything is a process**. Processes are isolated, concurrent
units of execution that may **only share data via message-passing**. Because of
the strong isolation between processes, each process can die, panic, crash,
throw, or anything else without directly affecting the integrity of the rest of
the runtime. This is the secret to Hearth's hot-reloading!

> TODO: make last sentence ^^^ *boring-er*. design docs aren't used for selling.
> TODO: emphasize practical precedence of this in Erlang's history

To receive messages from other processes, a process may create any number of
**mailboxes**. A mailbox may only receive messages from processes that own
a **capability** to a specific mailbox, so which processes can be trusted
to send messages to any given mailbox can be easily sandboxed. [Capability-
based security](https://en.wikipedia.org/wiki/Capability-based_security) is
the foundation for Hearth's security model, enabling us to confidently execute
untrusted guests without them gaining access to sensitive information or
privileges that are not explicitly shared with them.

Messages themselves contain a **plain string of bytes as their data payload**
as well as a list of zero or more capabilities. This is the mechanism through
which capabilities, and therefore access to any kind of resource within the
runtime, are shared among processes.

Processes can also observe when other processes' mailboxes become unavailable
by **monitoring** a mailbox's capability from another mailbox. When a monitored
mailbox is closed by its process, or the mailbox's process dies, the mailbox
monitoring it will receive a **"down signal"**.

When a pair of processes are dependent on each other for shared functionality,
they can be **linked**. When a process dies, all of the processes linked to it
die as well. Links are bidirectional; if a process links to another process,
either will die if the other dies. Linked processes may also be unlinked from
each other at any time.

The operations that a capability may be used to perform are limited by its
**permissions**. For example, for a capability to be used to send a message to
the capability's mailbox, that capability must have the "send" permission.
Processes are not permitted to add new permissions to capabilities, so if a
capability with no "send" permission is sent to a process, that process has no
means to acquire a means to send messages using that capability.

Hearth implements its low-level message-passing logic in a dedicated crate named
**Flue**. You can browse Flue's [source code](https://github.com/hearth-rs/flue)
or read its [API documentation](https://docs.rs/flue). It is highly recommended
to study how Flue works to understand how Hearth works as a whole, since Flue is
used throughout the entire host, and the guest API is directly connected to the
Flue API.

> TODO: rewrite last sentence: "A thorough understanding of Flue is essential to understanding how Hearth works as a whole..." or something (don't use passive voice)

- transparent message-passing over the network

# Native Resources

## Renderer

- based on rend3
- textures
- meshes
- objects
- lights

## Terminal

- necessary for bootstrapping self-modification
- interacts with the native OS
- integrates with CLI and TUI tools
- easier to implement than Wayland or an embedded web browser
- MSDF text rendering

## Window

- window commands
  - camera
- window events
  - redraw
  - keyboard input
  - mouse input

## Filesystem

## Time

## Daemon

## Canvas

- efficiently updated panels
- used to draw UIs and arbitrary 2D content

## Debugging

# Init System

- the initial guest process spawned by the host
- equivalent of PID 1 on Unix-likes
- starts up all other guest processes
- see [Kindling](#kindling) for more info

# IPC

- interfaces dynamic OS processes with Hearth processes
- platform-specific transport
  - UDS
  - Named Pipes
- implemented `hearth-daemon`

# Kindling

- init system
- user-facing features
- service-oriented architecture
- hot reload
- client/server support?

# CLI

# TUI

# Hibashi

# Design Patterns

## Registry

## Request-Response

## Ownership Via Links

## Sink

## Pub-Sub

## Factory

## Supervisor

# Guest Development

- guest crate
- kindling common code
  - host
  - utils
  - schema

# Host Development

- `hearth-runtime`
- plugins
- assets
