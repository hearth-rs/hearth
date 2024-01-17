> **WARNING**: the contents of this design document are currently out of date.
> Recently, Hearth has done a radical change from the "everyone is trusted"
> policy described in this document to a capability-based security model.
> Please refer to #104 for more info surrounding this. Because of this, Hearth
> has grown a lot in complexity and core architecture, and this documentation
> has not been updated to reflect this yet.
>
> Most of the ideas here are still relevant to Hearth's design, but please keep
> in mind that Hearth has undergone some fundamental restructuring since the
> specifics described here were written.
>
> Thank you for your interest in Hearth, and stay posted for new documentation!

# Architecture

The name "Hearth" is meant to invoke the coziness of sharing a warm fireplace
with loved ones. Hearth is based on a straight-forward client-server network
architecture, with multiple peers connecting to a single persistent host. In
Hearth, all peers on the network are assumed to be friendly, so any peer can
perform any action on the world. This assumption sidesteps a massive amount of
design dilemmas that have plagued virtual spaces for as long as they have
existed.

In order to upgrade Hearth's design from a trustful private environment to a
trustless public service, future systems will need to solve these dilemmas in
order to prevent griefing. Hearth's goal is to explore implementations of
shared execution and content workflow, not security or permissions systems.

Hearth is effectively two programs at once: a distributed scripting
environment and a game engine. The scripting provides the metatextual rules of
the environment and the game engine creates the spatial substrate of the world.
The bridge between distributed execution logic and game engine logic is in the
scripting API. Scripts perform high-level logic in the environment, but are
also singularly responsible for loading and managing all of the spatial
content.

## Network

Hearth operates over TCP socket connections. Although other real-time
networking options like GameNetworkingSockets or QUIC have significantly
better performance, TCP is being used because it doesn't require NAT traversal
and it performs packet ordering and error checking without help from userspace.

Hearth spaces are password-protected. Hearth assumes that peers are friendly,
but not that all network nodes with access to the server can become peers, or
that man-in-the-middle attacks are impossible. When a connection to a Hearth
server is made, the client and server execute a password-authenticated
key-exchange (PAKE) protocol, then use the resulting session key to encrypt
all future communication between them.

The specific PAKE method used is
[Facebook's OPAQUE protocol](https://github.com/facebook/opaque-ke) with
[Argon2](https://en.wikipedia.org/wiki/Argon2) as the password hash derivation
function. OPAQUE has been audited by NCC Group and Argon2 is well-established
for password hashing and is highly popular. After exchanging session keys,
[ChaCha20](https://en.wikipedia.org/wiki/Salsa20) is directly applied to TCP
communication. ChaCha20 is another well-established cryptography protocol.

## Scripting

Scripting in Hearth is done with [WebAssembly](https://webassembly.org/).
WebAssembly is extremely performant, simple,
[a compile target for lots of languages](https://github.com/appcypher/awesome-wasm-langs),
and has especially good support in Rust, Hearth's main development language.

Hearth's execution model is inspired by BEAM, the virtual machine that the
Erlang and Elixir programming languages run on. BEAM is a tried-and-true
solution for creating resilient, fault-tolerant, hot-swappable systems, so many
of Hearth's design choices follow BEAM's. Hearth scripts are ran as green
thread "processes" that can spawn and kill other processes. Processes share
data by sending messages back and forth from each other. A significant
departure from BEAM is that because Hearth runs on multiple computers at once,
processes can spawn other processes on different peers, which can be either
another client or the server. Hearth's networking implementation will then
transport messages sent from a process on one peer to another, and the server
will relay client-to-client messages.

A working example of a BEAM-like WebAssembly runtime is Lunatic. Lunatic runs
green Wasm threads on top of a Rust asynchronous runtime, which handles all of
the cooperative multitasking and async IO. Lunatic also implements preemptive
multitasking by timeslicing Wasm execution, then yielding to other green
threads. Lunatic's operation is another large influence on Hearth's design, and
Hearth may reference--or even directly use--Lunatic's code in its own codebase.

## Services

Processes may be registered in the runtime as a service. A service is simply a
process that can be located using a string identifier. This way, other
processes depending on the functionality provided by another process may
consistently acquire access to that process, even if the process ID for the
required process changes between instantiations, runtime executions, or peers.

## Native Processes

Some processes are not WebAssembly scripts but are instead processes running in
the runtime application. These native processes provide non-native processes
with features that sandboxed WebAssembly scripts would not otherwise have
access to. These include global space configuration, mouse and keyboard input
on desktops, and more.

## IPC

To administrate the network, every peer in a Hearth network exposes an IPC
interface on a Unix domain socket. This can be used to query running processes,
retrieve properties of the other peers on the network, send and receive
messages to processes, kill or restart misbehaving processes, and most
importantly, load new WebAssembly modules into the environment. The expected
content development loop in Hearth is to develop new scripts natively, then
load and execute the compiled module into Hearth using IPC.

## Rendering

## Lumps

Lumps are multipurpose, hash-identified binary blobs that are exchanged
on-demand over the Hearth network. To distribute lumps to other peers, Hearth
first hashes each lump and uses that hash as an identifier to other peers. If
a peer does not own a copy of a lump, it will not recognize the hash of that
lump in its cache, and it will request the lump's data from another peer.
Client peers request lump data from the server, and the server requests lump
data from the client that has referenced the unrecognized ID.

Processes may both create their own lump in memory and read a foreign lump's
data back into process memory. In this way, processes may procedurally generate
runtime Hearth content into the space, load content data formats that the
core Hearth runtime does not recognize, or pipeline lumps through multiple
processes that each perform some transformation on them.

Processes may send and receive the hashed IDs of lumps to other processes via
messages. When an ID is sent to a process on a remote peer, however, the
remote runtime may not recognize that a lump's ID has been referenced. The
result is that the remote process obtains an ID for a lump but no way to access
its data. To remedy this, processes have a host call that explicitly transfers
a lump's data to a remote peer. This way, processes can ensure that remote
processes have access to lumps that are being transferred.

Note that lumps can be created by processes and that Wasm modules are loaded
from lumps. As a consequence of this, Hearth processes may generate and load
Wasm modules at runtime. This allows a WebAssembly compiler that can create new
Hearth processes to be in of itself a Wasm Hearth process. A major field of
research in Hearth's [beta phase](#phase-3-beta) is to study the possibilities
of a self-hosting Hearth environment using this technique.

## Terminal Emulator

So far, only Hearth's network architecture, execution model, and content model
have been described. These fulfill the first and second principles of Hearth's
design philosophy, but not the third principle: the space itself must provide
tooling to extend and modify itself. The easiest and simplest way to do this is
to implement a virtual terminal emulator inside of the 3D space, as a floating
window. This terminal emulator runs a native shell on the hosting user's
computer, with full access to the filesystem, native programs, and the IPC
interface for the Hearth process. The user can use the virtual terminal to edit
Hearth scripts using an existing terminal text editor like Vim, Neovim, or
Emacs, compile the script into a WebAssembly module, then load and execute that
module, all without ever switching from Hearth to another application or
shutting down Hearth itself.

The terminal emulator's text is rendered with multichannel signed distance
field (MSDF) rendering. MSDF rendering is good for text in 3D space because
each glyph can be drawn with high-quality antialiasing and texture filtering
from a large variety of viewing angles.

## Platform

Hearth runs outside of the browser as a native application. Linux is the only
target platform to start with but Windows support will also be added during
the [alpha phase of development](#phase-2-alpha).

# Usecases

# Implementation

Hearth is written entirely in [Rust](https://www.rust-lang.org/), a memory-safe
compiled language with powerful generics and parallelization. Choosing Rust 
eases developing a native runtime with rapid iteration and portability in mind
without compromising on performance. Because Hearth uses WebAssembly, Rust can
be used to write Hearth processes too. This monolanguage setup reduces
development friction between guest and host code.

Hearth is licensed under the [GNU Affero General Public License version 3.0](https://www.gnu.org/licenses/agpl-3.0.html).
The AGPL is used for Hearth because it ensures that all peers on a network can
view the source code of all other peers' runtimes. Hearth's philosophy affirms
that there is no valid use of the Hearth runtime between unfriendly peers, and
if a peer won't share their source code with you, they're not your friend.

## Networking

The [Tokio](https://tokio.rs) provides an asynchronous Rust runtime for async
code, non-blocking IO over both TCP and Unix domain sockets, and a foundation
for WebAssembly multitasking.

The protocols for both IPC and client-server communication are based on the
[Remoc](https://docs.rs/remoc/) crate, which implements transport-agnostic
channels, binary blob transport, remote procedure calls (RPC), watchable data
collections, and other networking constructs. This will save a lot of
development time that would otherwise be spent writing networking primitives.
Using Remoc makes it difficult to standardize a top-to-bottom protocol
definition, and the assumption is being made that no programs other than Hearth
or its forks will be using the protocol. The development time saved is more
than worth the loss of interoperability.

## Plugins

Although Hearth's goal is to implement as much as possible inside of its
processes, some system- or hardware-level interfacing is unavoidable. In
order to make this interfacing modular and optional Hearth defines a plugin
system for all non-essential components that all of its major subsystems are
built on top of. This includes IPC, rendering, input handling, terminal
management, and WebAssembly process execution itself.

## Assets

Assets are specialized host-side objects that are loaded from lump data.
Examples of assets are WebAssembly modules, 3D meshes, images, and other
formatted media that's used by host-side plugins. To load a lump, each plugin
may register asset loaders into the runtime on initialization. Each asset
loader defines a function that takes a binary blob as input and returns an
asset object. Then, the runtime's asset store returns a shared pointer to that
new asset object. Assets are cached by lump ID in the asset store, and old,
unused lumps are freed to conserve system memory usage. The primary purpose of
the lump system is to provide native plugin authors with a reusable utility for
loading serialized data and objects from lumps.

## Rendering

Scenes are rendered with [rend3](https://github.com/BVE-Reborn/rend3), a
batteries-included rendering engine based on
[wgpu](https://github.com/gfx-rs/wgpu). rend3 includes PBR, lighting, a render
graph, skybox rendering, frustum culling, rigged mesh skinning, tone mapping,
and other handy renderer features that Hearth doesn't need to do itself.
Additionally, rend3's frame graph allows us to easily extend the renderer with
new features as needed.

## Terminal Emulator

The terminal emulator is implemented with the help of the
[alacritty_terminal](https://crates.io/crates/alacritty_terminal) crate, which
parses the output of native child processes and writes it into a display-ready
grid of characters. Then, Hearth draws the characters onto 2D planes projected
in 3D space using MSDF textures generated with the
[msdfgen](https://crates.io/crates/msdfgen) crate. This rendering is a custom
node in the rend3 frame graph with a custom shader.

- TBD: can we load glyph outlines from a TTF file without building Freetype?
  * note: the ttf-parser crate looks like what we need)
- TBD: UX for interacting with in-space terminals?

## Input

## WebAssembly

### Loading Lumps

### Logging From Wasm

## CLI

Hearth provides a command-line interface (CLI) utility program to perform
simple interactions with the daemon. This may be invoked with an interactive
shell or by shell scripts. Scripts may chain together multiple invocations of
the CLI to create more complex workflows. This allows Hearth users to
repeatedly perform complex actions without needing to compile and spawn a
new Hearth process or to compile and run a new native process.

The CLI program is named `hearth-ctl` and has multiple subcommands that each
perform a different operation on the runtime. Hearth may add more subcommands
as needed, but at the current time of writing it is known that they include:

- `load-lump`: load a lump
- `list-processes`: list processes
- `kill`: kill a process
- `spawn`: spawn a process
- `tail`: tail a process's log
- `send`: send a process a message

`hearth-ctl` follows POSIX-like conventions for user interaction. It returns
reasonable exit codes according to the POSIX standard, and displays output to
`stdout` in lightly- or un-formatted strings that can be easily processed by a
shell script.

Exit codes are provided by the
[yacexits](https://crates.io/crates/yacexits) crate.

## TUI

A more advanced alternative to `hearth-ctl` is `hearth-console`, a terminal
user interface (TUI) utility program. `hearth-console` provides a long-running
live view into the status of the connected Hearth daemon, and user-friendly
controls (at least for users acclimated to GUIs) to administrate the Hearth
runtime.

Features include, but are not limited to:
- a live, `htop`-like process view displaying all processes and services (TODO: and their children?)
- tabbed process logs to follow multiple process logs simultaneously
- TODO: resource consumption?

`hearth-console` uses the [tui](https://crates.io/crates/tui) crate as the
base TUI framework.

# Roadmap

## Phase 0: Pre-Production

In phase 0, Hearth documents its purpose, proposes an implementation, decides
on which libraries and resources to use in its development, and finds a handful
of core developers who understand Hearth's goals and who are capable of
meaningfully contributing in the long run.

- [ ] write a design document
- [x] create a Discord server
- [x] create a GitHub repository
- [ ] onboard 3-4 core developers who can contribute to Hearth long-term
- [x] design a project logo
- [x] set up continuous integration to check pull requests
- [x] write a CONTRIBUTORS.md describing contribution workflow
- [x] design a workspace structure
- [x] set up licensing headers and copyright information
- [ ] finalize the rest of the phases of the roadmap
- [ ] money?

## Phase 1: Pre-Alpha

In phase 1, each subsystem of Hearth is developed, and the details of its
design aspects are made concrete. The whole system has not yet been tied
together, and low-level design decisions are considered in isolation of each
other.

Hearth's core host-side components can generally be decoupled from each other
into several different areas of development or subsystems:

1. IPC, TUI, and CLI interfaces.
2. Client-server networking.
3. Process management.
4. Virtual terminal emulator development.

Because these different areas are independent, the goal is to work on each of
these areas in parallel. During this point of development, it's important that
multiple developers work in coordination with each other in order to progress
to alpha as quickly as possible. Mock interfaces and placeholder data where
functioning inter-component code would otherwise go are used to develop each
component separately.

- [x] implement password authentication and stream encryption
- [x] create a standalone, usable, rend3-based 3D terminal emulator
- [x] design an inter-subsystem plugin interface
- [x] create a process store capable of sending messages between local processes
- [ ] implement process linking
- [x] create a lump store data structure
- [x] create an asset loading system
- [x] design initial RPC network interfaces
- [ ] write mock RPC endpoints for testing subsystems in isolation
- [x] implement IPC using Unix domain sockets (Unix only)
- [ ] complete `hearth-ctl`
- [ ] define guest-to-host WebAssembly APIs for logging, lump loading, and message transmission
- [x] create a native service for spawning WebAssembly processes
- [x] integrate rend3 and winit into `hearth-client`
- [ ] use WebSockets (optionally over TLS) for networking

## Phase 2: Alpha

In phase 2, Hearth begins to come together as a whole. Each subsystem is hooked
into the others, and the developers work together to synthesize their work into
a single functioning application. Although at this point in development network
servers are started up for testing, the protocols between subsystems are
highly unstable, so long-lived, self-sustaining virtual spaces are still
unfeasible.

- [ ] write a unit test suite for Wasm guests written in Rust
- [ ] implement an asset reaper for unused assets
- [ ] implement message-sending between processes on different peers
- [ ] implement a process supervision tree in `hearth-guest`
- [x] asynchronous MSDF glyph loading for large fonts
- [ ] support IPC on Windows using an appropriate alternative to Unix domain sockets
- [ ] complete the WebAssembly host call APIs
- [ ] complete `hearth-console`
- [ ] add asset loaders for rend3 meshes, 2D textures, cube textures, and materials
- [ ] integrate `alacritty_terminal` with Tokio's child process API
- [ ] create native services for rend3 meshes, lights, and skeletons
- [ ] create native services for pancake mode input handling
- [ ] create native services for rend3 configuration like skyboxes, global lighting, and camera setup
- [ ] create native services for virtual terminal management
- [ ] create a server blocklist and allowlist system

## Phase 3: Beta

In phase 3, Hearth's protocols and system interfaces are mature and relatively
stable, so a long-lived development space is created. In this space, developers
work together on exploring the capabilities of Hearth processes, and implement
practical applications in Hearth using Hearth's fundamental toolkit. If
oversights or missing features are found in Hearth's interfaces, they are
addressed as fit. However, because the fundamentals of Hearth's implementation
are complete, changes to interfaces are infrequent and often non-breaking.

A major focus of this phase is to refine the design principles of writing
Hearth processes through rapid iteration, collaboration, and peer review. This
makes phase 3 the most difficult phase to complete, as Hearth's goal during
this step is to explore uncharted design territory in a unique execution
environment.

Here are some ideas for subjects of exploration that Hearth may explore in
beta:
- Database
  - data backup
  - process-to-host integration with database APIs
  - persistent world storage
- Physics
  - avatar movement and input handling systems
  - guest-side physics engines (using [Rapier](https://rapier.rs))
  - avatar skeletal animation
  - inverse kinematics
- Models
  - OBJ loading
  - FBX loading
  - glTF loading
- Audio
  - audio compression
  - spatial audio
  - voice chat
- Editing
  - collaborative world editing
  - live mesh editing
  - live interior design and virtual architecture tooling
- Languages/Scripting
  - WASI-based text editors for non-native script authoring
  - Wasm compilers in Hearth for non-native script development
  - guest APIs for more WebAssembly languages (i.e. C/C++, AssemblyScript, 
Grain)
  - non-Wasm process scripting runtimes (i.e. Lua, Mono, Javascript, Lisp, 
Python)
  - Create a block-based [Scratch](https://scratch.mit.edu)-like scripting language
- Miscellaneous
  - in-space virtual cameras for external applications to record the space 
through

These topics may be further explored post-beta. They mainly serve the purpose
of guiding Hearth's developers towards supporting an aligned set of expected
usecases and to fuel curiosity into Hearth's potential.

## Phase 4: Release

- [ ] publish Hearth on the AUR
- [ ] publish Hearth's crates to the AUR
- [ ] evaluate Hearth's design and brainstorm future improvements to found problems
- [ ] create a launcher for Hearth server and client
- [ ] create comprehensive documentation on usage
- [ ] create a web page for promoting and reusing community contributions
