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

use std::{sync::Arc, time::Instant};

use glam::{dvec2, uvec2, Mat4};
use hearth_rend3::{
    rend3::{
        self,
        types::{Camera, CameraProjection},
    },
    wgpu, FrameRequest, Rend3Plugin,
};
use hearth_runtime::{
    async_trait,
    flue::{CapabilityRef, Permissions},
    hearth_macros::GetProcessMetadata,
    hearth_schema::window::*,
    runtime::{Plugin, RuntimeBuilder},
    utils::{MessageInfo, PubSub, ServiceRunner, SinkProcess},
};
use rend3::InstanceAdapterDevice;
use tokio::sync::{mpsc, oneshot};
use tracing::warn;
use winit::{
    event::{DeviceEvent, Event, WindowEvent as WinitWindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    window::{Window as WinitWindow, WindowBuilder},
};

/// A message sent from the rest of the program to a window.
#[derive(Clone, Debug)]
pub enum WindowRxMessage {
    /// Update the title.
    SetTitle(String),

    /// Set the cursor grab mode.
    SetCursorGrab(CursorGrabMode),

    /// Set the cursor visibility.
    SetCursorVisible(bool),

    /// Update the renderer camera.
    SetCamera {
        /// Vertical field of view in degrees.
        vfov: f32,

        /// Near plane distance. All projection uses an infinite far plane.
        near: f32,

        /// The camera's view matrix.
        view: Mat4,
    },

    /// Broadcast the current state of the window to all event subscribers.
    BroadcastState,

    /// The window is requested to quit.
    Quit,
}

/// A message sent from a window to the rest of the program.
#[derive(Clone, Debug)]
pub enum WindowTxMessage {
    /// The window has been requested to quit.
    Quit,
}

/// Message sent from the window on initialization.
pub struct WindowOffer {
    /// A sender of [WindowRxMessage] to this window.
    pub incoming: EventLoopProxy<WindowRxMessage>,

    /// A receiver for [WindowTxMessage] from the window.
    pub outgoing: mpsc::UnboundedReceiver<WindowTxMessage>,

    /// A [Rend3Plugin] compatible with this window.
    pub rend3_plugin: Rend3Plugin,

    /// The [WindowPlugin] for this window.
    pub window_plugin: WindowPlugin,
}

/// A single running desktop window.
struct Window {
    /// Sender to outgoing window events.
    outgoing_tx: mpsc::UnboundedSender<WindowTxMessage>,

    /// The inner winit window.
    window: WinitWindow,

    /// The wgpu instance, adapter, and device compatible with this window.
    iad: InstanceAdapterDevice,

    /// This window's wgpu surface.
    surface: Arc<wgpu::Surface>,

    /// This window's wgpu surface configuration.
    config: wgpu::SurfaceConfiguration,

    /// Sender of frame requests to the rend3 renderer.
    frame_request_tx: mpsc::UnboundedSender<FrameRequest>,

    /// This window's current camera in the rend3 world..
    camera: Camera,

    /// Outgoing window events.
    events_tx: mpsc::UnboundedSender<WindowEvent>,

    /// Tracks the last redraw to this window.
    last_redraw: Instant,
}

impl Window {
    async fn new(event_loop: &EventLoop<WindowRxMessage>) -> (Self, WindowOffer) {
        let window = WindowBuilder::new()
            .with_title("Hearth Client")
            .with_inner_size(winit::dpi::LogicalSize::new(128.0, 128.0))
            .build(event_loop)
            .unwrap();

        let size = window.inner_size();
        let swapchain_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let iad = rend3::create_iad(None, None, None, None).await.unwrap();
        let surface = unsafe { iad.instance.create_surface(&window) };
        let surface = Arc::new(surface);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };

        surface.configure(&iad.device, &config);
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        let rend3_plugin = Rend3Plugin::new(iad.to_owned(), swapchain_format);
        let frame_request_tx = rend3_plugin.frame_request_tx.clone();
        let (events_tx, events_rx) = mpsc::unbounded_channel();

        let window = Self {
            outgoing_tx,
            window,
            iad,
            surface,
            config,
            camera: Camera::default(),
            frame_request_tx,
            events_tx,
            last_redraw: Instant::now(),
        };

        let window_plugin = WindowPlugin {
            incoming: event_loop.create_proxy(),
            events_rx,
        };

        let offer = WindowOffer {
            incoming: event_loop.create_proxy(),
            outgoing: outgoing_rx,
            rend3_plugin,
            window_plugin,
        };

        (window, offer)
    }

    pub fn on_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.iad.device, &self.config);
        self.window.request_redraw();
    }

    pub fn on_draw(&mut self) {
        // notify redraw event
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_redraw);
        let dt = elapsed.as_secs_f32();
        self.notify_event(WindowEvent::Redraw { dt });
        self.last_redraw = now;

        let output_frame = rend3::util::output::OutputFrame::Surface {
            surface: self.surface.to_owned(),
        };

        let resolution = glam::UVec2::new(self.config.width, self.config.height);

        let (on_complete, on_complete_rx) = oneshot::channel();

        let request = FrameRequest {
            output_frame,
            camera: self.camera,
            resolution,
            on_complete,
        };

        if self.frame_request_tx.send(request).is_err() {
            tracing::warn!("failed to request frame");
        } else {
            let _ = on_complete_rx.blocking_recv();
        }

        self.window.request_redraw();
    }

    pub fn on_event(&mut self, event: &WinitWindowEvent) -> bool {
        match event {
            WinitWindowEvent::Resized(size) => {
                self.on_resize(*size);
                self.notify_event(WindowEvent::Resized(uvec2(size.width, size.height)));
            }
            WinitWindowEvent::ReceivedCharacter(c) => {
                self.notify_event(WindowEvent::ReceivedCharacter(*c));
            }
            WinitWindowEvent::Focused(focus) => {
                self.notify_event(WindowEvent::Focused(*focus));
            }
            WinitWindowEvent::KeyboardInput {
                input,
                is_synthetic,
                ..
            } => {
                self.notify_event(WindowEvent::KeyboardInput {
                    input: KeyboardInput {
                        scancode: input.scancode,
                        state: conv_element_state(input.state),
                        virtual_keycode: input.virtual_keycode.map(conv_keycode),
                    },
                    is_synthetic: *is_synthetic,
                });
            }
            WinitWindowEvent::ModifiersChanged(modifiers) => {
                self.notify_event(WindowEvent::ModifiersChanged(
                    ModifiersState::from_bits(modifiers.bits()).unwrap(),
                ));
            }
            WinitWindowEvent::CursorMoved { position, .. } => {
                self.notify_event(WindowEvent::CursorMoved {
                    position: dvec2(position.x, position.y),
                });
            }
            WinitWindowEvent::CursorEntered { .. } => {
                self.notify_event(WindowEvent::CursorEntered {});
            }
            WinitWindowEvent::CursorLeft { .. } => {
                self.notify_event(WindowEvent::CursorLeft {});
            }
            WinitWindowEvent::MouseWheel { delta, phase, .. } => {
                self.notify_event(WindowEvent::MouseWheel {
                    delta: conv_scroll_delta(*delta),
                    phase: conv_touch_phase(*phase),
                });
            }
            WinitWindowEvent::MouseInput { state, button, .. } => {
                self.notify_event(WindowEvent::MouseInput {
                    state: conv_element_state(*state),
                    button: conv_mouse_button(*button),
                });
            }
            WinitWindowEvent::CloseRequested => {
                self.outgoing_tx.send(WindowTxMessage::Quit).unwrap();
                return true;
            }
            WinitWindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => {
                self.on_resize(**new_inner_size);

                self.notify_event(WindowEvent::ScaleFactorChanged {
                    scale_factor: *scale_factor,
                    new_inner_size: uvec2(new_inner_size.width, new_inner_size.height),
                });
            }
            _ => {}
        }

        false
    }

    pub fn notify_event(&self, event: WindowEvent) {
        let _ = self.events_tx.send(event);
    }

    pub fn broadcast_state(&self) {
        let size = self.window.inner_size();
        let size = uvec2(size.width, size.height);
        self.notify_event(WindowEvent::Resized(size));

        let scale_factor = self.window.scale_factor();
        self.notify_event(WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size: size,
        });
    }
}

pub struct WindowCtx {
    event_loop: EventLoop<WindowRxMessage>,
    window: Window,
}

impl WindowCtx {
    pub async fn new() -> (Self, WindowOffer) {
        let event_loop = EventLoopBuilder::with_user_event().build();
        let (window, offer) = Window::new(&event_loop).await;
        (Self { event_loop, window }, offer)
    }

    pub fn run(self) -> ! {
        let Self {
            event_loop,
            mut window,
        } = self;

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::WindowEvent { ref event, .. } => {
                    if window.on_event(event) {
                        control_flow.set_exit();
                    }
                }
                Event::MainEventsCleared => {
                    window.window.request_redraw();
                }
                Event::RedrawRequested(_) => {
                    window.on_draw();
                }
                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta },
                    ..
                } => {
                    window.notify_event(WindowEvent::MouseMotion(delta.into()));
                }
                Event::UserEvent(event) => match event {
                    WindowRxMessage::SetTitle(title) => window.window.set_title(&title),
                    WindowRxMessage::SetCursorGrab(mode) => {
                        // convert from guest type to native type
                        use winit::window::CursorGrabMode as Winit;
                        use CursorGrabMode::*;
                        let mode = match mode {
                            None => Winit::None,
                            Confined => Winit::Confined,
                            Locked => Winit::Locked,
                        };

                        if let Err(err) = window.window.set_cursor_grab(mode) {
                            warn!("set cursor grab error: {err:?}");
                        }
                    }
                    WindowRxMessage::SetCursorVisible(visible) => {
                        window.window.set_cursor_visible(visible)
                    }
                    WindowRxMessage::SetCamera { vfov, near, view } => {
                        window.camera = Camera {
                            projection: CameraProjection::Perspective { vfov, near },
                            view,
                        }
                    }
                    WindowRxMessage::BroadcastState => window.broadcast_state(),
                    WindowRxMessage::Quit => control_flow.set_exit(),
                },
                _ => (),
            }
        });
    }
}

/// A plugin that provides native window access to guests.
pub struct WindowPlugin {
    incoming: EventLoopProxy<WindowRxMessage>,
    events_rx: mpsc::UnboundedReceiver<WindowEvent>,
}

impl Plugin for WindowPlugin {
    fn finalize(mut self, builder: &mut RuntimeBuilder) {
        let pubsub = Arc::new(PubSub::new(builder.get_post()));

        tokio::spawn({
            let pubsub = pubsub.clone();
            async move {
                while let Some(event) = self.events_rx.recv().await {
                    pubsub.notify(&event).await;
                }
            }
        });

        builder.add_plugin(WindowService {
            incoming: self.incoming,
            pubsub,
        });
    }
}

/// The native window service. Accepts WindowRequest.
#[derive(GetProcessMetadata)]
pub struct WindowService {
    incoming: EventLoopProxy<WindowRxMessage>,
    pubsub: Arc<PubSub<WindowEvent>>,
}

#[async_trait]
impl SinkProcess for WindowService {
    type Message = WindowCommand;

    async fn on_message<'a>(&'a mut self, message: MessageInfo<'a, WindowCommand>) {
        let send = |event| {
            self.incoming.send_event(event).unwrap();
        };

        use WindowCommand::*;
        match message.data {
            Subscribe => {
                let Some(sub) = message.caps.get(0) else {
                    warn!("Subscribe messsage is missing capability");
                    return;
                };

                if sub.get_permissions().contains(Permissions::MONITOR) {
                    sub.monitor(message.process.borrow_parent()).unwrap();
                }

                self.pubsub.subscribe(sub.clone());

                send(WindowRxMessage::BroadcastState);
            }
            Unsubscribe => {
                let Some(sub) = message.caps.get(0) else {
                    warn!("Unsubscribe messsage is missing capability");
                    return;
                };

                self.pubsub.unsubscribe(sub.clone());
            }
            SetTitle(title) => send(WindowRxMessage::SetTitle(title)),
            SetCursorGrab(grab) => send(WindowRxMessage::SetCursorGrab(grab)),
            SetCursorVisible(visible) => send(WindowRxMessage::SetCursorVisible(visible)),
            SetCamera { vfov, near, view } => send(WindowRxMessage::SetCamera { vfov, near, view }),
        }
    }

    async fn on_down<'a>(&'a mut self, cap: CapabilityRef<'a>) {
        self.pubsub.unsubscribe(cap);
    }
}

impl ServiceRunner for WindowService {
    const NAME: &'static str = SERVICE_NAME;
}

fn conv_element_state(state: winit::event::ElementState) -> ElementState {
    use winit::event::ElementState as Winit;
    use ElementState as Schema;
    match state {
        Winit::Pressed => Schema::Pressed,
        Winit::Released => Schema::Released,
    }
}

fn conv_scroll_delta(delta: winit::event::MouseScrollDelta) -> MouseScrollDelta {
    use winit::event::MouseScrollDelta as Winit;
    use MouseScrollDelta as Schema;
    match delta {
        Winit::LineDelta(x, y) => Schema::LineDelta(x, y),
        Winit::PixelDelta(pos) => Schema::PixelDelta(dvec2(pos.x, pos.y)),
    }
}

fn conv_touch_phase(phase: winit::event::TouchPhase) -> TouchPhase {
    use winit::event::TouchPhase as Winit;
    use TouchPhase as Schema;
    match phase {
        Winit::Started => Schema::Started,
        Winit::Moved => Schema::Moved,
        Winit::Ended => Schema::Ended,
        Winit::Cancelled => Schema::Cancelled,
    }
}

fn conv_mouse_button(button: winit::event::MouseButton) -> MouseButton {
    use winit::event::MouseButton as Winit;
    use MouseButton as Schema;
    match button {
        Winit::Left => Schema::Left,
        Winit::Right => Schema::Right,
        Winit::Middle => Schema::Middle,
        Winit::Other(id) => Schema::Other(id),
    }
}

fn conv_keycode(code: winit::event::VirtualKeyCode) -> VirtualKeyCode {
    use winit::event::VirtualKeyCode as Winit;
    use VirtualKeyCode as Schema;
    match code {
        Winit::Key1 => Schema::Key1,
        Winit::Key2 => Schema::Key2,
        Winit::Key3 => Schema::Key3,
        Winit::Key4 => Schema::Key4,
        Winit::Key5 => Schema::Key5,
        Winit::Key6 => Schema::Key6,
        Winit::Key7 => Schema::Key7,
        Winit::Key8 => Schema::Key8,
        Winit::Key9 => Schema::Key9,
        Winit::Key0 => Schema::Key0,
        Winit::A => Schema::A,
        Winit::B => Schema::B,
        Winit::C => Schema::C,
        Winit::D => Schema::D,
        Winit::E => Schema::E,
        Winit::F => Schema::F,
        Winit::G => Schema::G,
        Winit::H => Schema::H,
        Winit::I => Schema::I,
        Winit::J => Schema::J,
        Winit::K => Schema::K,
        Winit::L => Schema::L,
        Winit::M => Schema::M,
        Winit::N => Schema::N,
        Winit::O => Schema::O,
        Winit::P => Schema::P,
        Winit::Q => Schema::Q,
        Winit::R => Schema::R,
        Winit::S => Schema::S,
        Winit::T => Schema::T,
        Winit::U => Schema::U,
        Winit::V => Schema::V,
        Winit::W => Schema::W,
        Winit::X => Schema::X,
        Winit::Y => Schema::Y,
        Winit::Z => Schema::Z,
        Winit::Escape => Schema::Escape,
        Winit::F1 => Schema::F1,
        Winit::F2 => Schema::F2,
        Winit::F3 => Schema::F3,
        Winit::F4 => Schema::F4,
        Winit::F5 => Schema::F5,
        Winit::F6 => Schema::F6,
        Winit::F7 => Schema::F7,
        Winit::F8 => Schema::F8,
        Winit::F9 => Schema::F9,
        Winit::F10 => Schema::F10,
        Winit::F11 => Schema::F11,
        Winit::F12 => Schema::F12,
        Winit::F13 => Schema::F13,
        Winit::F14 => Schema::F14,
        Winit::F15 => Schema::F15,
        Winit::F16 => Schema::F16,
        Winit::F17 => Schema::F17,
        Winit::F18 => Schema::F18,
        Winit::F19 => Schema::F19,
        Winit::F20 => Schema::F20,
        Winit::F21 => Schema::F21,
        Winit::F22 => Schema::F22,
        Winit::F23 => Schema::F23,
        Winit::F24 => Schema::F24,
        Winit::Snapshot => Schema::Snapshot,
        Winit::Scroll => Schema::Scroll,
        Winit::Pause => Schema::Pause,
        Winit::Insert => Schema::Insert,
        Winit::Home => Schema::Home,
        Winit::Delete => Schema::Delete,
        Winit::End => Schema::End,
        Winit::PageDown => Schema::PageDown,
        Winit::PageUp => Schema::PageUp,
        Winit::Left => Schema::Left,
        Winit::Up => Schema::Up,
        Winit::Right => Schema::Right,
        Winit::Down => Schema::Down,
        Winit::Back => Schema::Back,
        Winit::Return => Schema::Return,
        Winit::Space => Schema::Space,
        Winit::Compose => Schema::Compose,
        Winit::Caret => Schema::Caret,
        Winit::Numlock => Schema::Numlock,
        Winit::Numpad0 => Schema::Numpad0,
        Winit::Numpad1 => Schema::Numpad1,
        Winit::Numpad2 => Schema::Numpad2,
        Winit::Numpad3 => Schema::Numpad3,
        Winit::Numpad4 => Schema::Numpad4,
        Winit::Numpad5 => Schema::Numpad5,
        Winit::Numpad6 => Schema::Numpad6,
        Winit::Numpad7 => Schema::Numpad7,
        Winit::Numpad8 => Schema::Numpad8,
        Winit::Numpad9 => Schema::Numpad9,
        Winit::NumpadAdd => Schema::NumpadAdd,
        Winit::NumpadDivide => Schema::NumpadDivide,
        Winit::NumpadDecimal => Schema::NumpadDecimal,
        Winit::NumpadComma => Schema::NumpadComma,
        Winit::NumpadEnter => Schema::NumpadEnter,
        Winit::NumpadEquals => Schema::NumpadEquals,
        Winit::NumpadMultiply => Schema::NumpadMultiply,
        Winit::NumpadSubtract => Schema::NumpadSubtract,
        Winit::AbntC1 => Schema::AbntC1,
        Winit::AbntC2 => Schema::AbntC2,
        Winit::Apostrophe => Schema::Apostrophe,
        Winit::Apps => Schema::Apps,
        Winit::Asterisk => Schema::Asterisk,
        Winit::At => Schema::At,
        Winit::Ax => Schema::Ax,
        Winit::Backslash => Schema::Backslash,
        Winit::Calculator => Schema::Calculator,
        Winit::Capital => Schema::Capital,
        Winit::Colon => Schema::Colon,
        Winit::Comma => Schema::Comma,
        Winit::Convert => Schema::Convert,
        Winit::Equals => Schema::Equals,
        Winit::Grave => Schema::Grave,
        Winit::Kana => Schema::Kana,
        Winit::Kanji => Schema::Kanji,
        Winit::LAlt => Schema::LAlt,
        Winit::LBracket => Schema::LBracket,
        Winit::LControl => Schema::LControl,
        Winit::LShift => Schema::LShift,
        Winit::LWin => Schema::LWin,
        Winit::Mail => Schema::Mail,
        Winit::MediaSelect => Schema::MediaSelect,
        Winit::MediaStop => Schema::MediaStop,
        Winit::Minus => Schema::Minus,
        Winit::Mute => Schema::Mute,
        Winit::MyComputer => Schema::MyComputer,
        Winit::NavigateForward => Schema::NavigateForward,
        Winit::NavigateBackward => Schema::NavigateBackward,
        Winit::NextTrack => Schema::NextTrack,
        Winit::NoConvert => Schema::NoConvert,
        Winit::OEM102 => Schema::OEM102,
        Winit::Period => Schema::Period,
        Winit::PlayPause => Schema::PlayPause,
        Winit::Plus => Schema::Plus,
        Winit::Power => Schema::Power,
        Winit::PrevTrack => Schema::PrevTrack,
        Winit::RAlt => Schema::RAlt,
        Winit::RBracket => Schema::RBracket,
        Winit::RControl => Schema::RControl,
        Winit::RShift => Schema::RShift,
        Winit::RWin => Schema::RWin,
        Winit::Semicolon => Schema::Semicolon,
        Winit::Slash => Schema::Slash,
        Winit::Sleep => Schema::Sleep,
        Winit::Stop => Schema::Stop,
        Winit::Sysrq => Schema::Sysrq,
        Winit::Tab => Schema::Tab,
        Winit::Underline => Schema::Underline,
        Winit::Unlabeled => Schema::Unlabeled,
        Winit::VolumeDown => Schema::VolumeDown,
        Winit::VolumeUp => Schema::VolumeUp,
        Winit::Wake => Schema::Wake,
        Winit::WebBack => Schema::WebBack,
        Winit::WebFavorites => Schema::WebFavorites,
        Winit::WebForward => Schema::WebForward,
        Winit::WebHome => Schema::WebHome,
        Winit::WebRefresh => Schema::WebRefresh,
        Winit::WebSearch => Schema::WebSearch,
        Winit::WebStop => Schema::WebStop,
        Winit::Yen => Schema::Yen,
        Winit::Copy => Schema::Copy,
        Winit::Paste => Schema::Paste,
        Winit::Cut => Schema::Cut,
    }
}
