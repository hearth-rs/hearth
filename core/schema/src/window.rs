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

//! **NOTE**: Most of these types are copied directly from the winit source
//! code, which is what this windowing protocol connects to on the native side.
//! winit is licensed under the Apache license, which can be found
//! [here](https://www.apache.org/licenses/LICENSE-2.0), and the original
//! source can be found [here](https://github.com/rust-windowing/winit). If
//! there is a more appropriate way to reuse these type definitions, please
//! open an issue and let us know!

use glam::{DVec2, Mat4, UVec2};
use serde::{Deserialize, Serialize};

/// The name of the service that provides the main client window.
pub const SERVICE_NAME: &str = "hearth.Window";

/// An event on the sender's window.
///
/// Refer to https://docs.rs/winit/latest/winit/event/enum.WindowEvent.html for
/// more info. This enum reimplements it since the original type does not
/// implement De/Serialize.
///
/// If something is missing, wrong, or otherwise broken, please open an issue.
// TODO file dropping/hovering?
// TODO IME support?
// TODO touchpad support?
// TODO touch support?
// TODO port DeviceId?
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum WindowEvent {
    /// The window has redrawn.
    Redraw {
        /// The time, in seconds, since the last redraw.
        dt: f32,
    },

    /// The window has resized. The new size is in physical display units.
    Resized(UVec2),
    ReceivedCharacter(char),
    Focused(bool),
    KeyboardInput {
        input: KeyboardInput,
        is_synthetic: bool,
    },
    ModifiersChanged(ModifiersState),
    CursorMoved {
        /// New position of the cursor in physical display units.
        position: DVec2,
    },
    CursorEntered {},
    CursorLeft {},
    MouseWheel {
        delta: MouseScrollDelta,
        phase: TouchPhase,
    },
    MouseInput {
        state: ElementState,
        button: MouseButton,
    },
    ScaleFactorChanged {
        scale_factor: f64,

        /// The new inner size of the window in physical display units.
        new_inner_size: UVec2,
    },

    /// Raw, unfiltered physical motion from a mouse device in unspecified units.
    MouseMotion(DVec2),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum WindowCommand {
    /// Subscribes to all [WindowEvents][WindowEvent] on this window using the
    /// first attached capability.
    ///
    /// If the capability has the monitor permission, it will be automatically
    /// unsubscribed when down.
    Subscribe, // and hit that bell

    /// Unbsubscribes from window events using the first attached capability.
    Unsubscribe,

    /// Sets the title of the window.
    SetTitle(String),

    /// Sets the grabbing mode of the cursor.
    SetCursorGrab(CursorGrabMode),

    /// Sets the visibility of the cursor.
    SetCursorVisible(bool),

    /// Updates the window's rendering camera.
    SetCamera {
        /// Vertical field of view in degrees.
        vfov: f32,

        /// Near plane distance. All projection uses an infinite far plane.
        near: f32,

        /// The camera's view matrix.
        view: Mat4,
    },
}

/// Describes a keyboard input event.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct KeyboardInput {
    /// Identifies the physical key being pressed. Hardware-dependent.
    pub scancode: u32,

    /// The new state of the key.
    pub state: ElementState,

    /// Identifies the semantic meaning of the key.
    pub virtual_keycode: Option<VirtualKeyCode>,
}

/// Describes touch-screen input state.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

/// The state of an input element such as a key or mouse button.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum ElementState {
    Pressed,
    Released,
}

/// Describes a button of a mouse controller.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

/// Describes a difference in the mouse scroll wheel state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MouseScrollDelta {
    /// Amount in lines or rows to scroll in the horizontal
    /// and vertical directions.
    ///
    /// Positive values indicate that the content that is being scrolled should move
    /// right and down (revealing more content left and up).
    LineDelta(f32, f32),

    /// Amount in pixels to scroll in the horizontal and
    /// vertical direction.
    ///
    /// Scroll events are expressed as a `PixelDelta` if
    /// supported by the device (eg. a touchpad) and
    /// platform.
    ///
    /// Positive values indicate that the content being scrolled should
    /// move right/down.
    ///
    /// For a 'natural scrolling' touch pad (that acts like a touch screen)
    /// this means moving your fingers right and down should give positive values,
    /// and move the content right and down (to reveal more things left and up).
    PixelDelta(DVec2),
}

/// Symbolic name for a keyboard key.
#[derive(Clone, Copy, Debug, Hash, Ord, PartialOrd, PartialEq, Eq, Deserialize, Serialize)]
pub enum VirtualKeyCode {
    /// The '1' key over the letters.
    Key1,
    /// The '2' key over the letters.
    Key2,
    /// The '3' key over the letters.
    Key3,
    /// The '4' key over the letters.
    Key4,
    /// The '5' key over the letters.
    Key5,
    /// The '6' key over the letters.
    Key6,
    /// The '7' key over the letters.
    Key7,
    /// The '8' key over the letters.
    Key8,
    /// The '9' key over the letters.
    Key9,
    /// The '0' key over the 'O' and 'P' keys.
    Key0,

    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    /// The Escape key, next to F1.
    Escape,

    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,

    /// Print Screen/SysRq.
    Snapshot,
    /// Scroll Lock.
    Scroll,
    /// Pause/Break key, next to Scroll lock.
    Pause,

    /// `Insert`, next to Backspace.
    Insert,
    Home,
    Delete,
    End,
    PageDown,
    PageUp,

    Left,
    Up,
    Right,
    Down,

    /// The Backspace key, right over Enter.
    Back,
    /// The Enter key.
    Return,
    /// The space bar.
    Space,

    /// The "Compose" key on Linux.
    Compose,

    Caret,

    Numlock,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadDivide,
    NumpadDecimal,
    NumpadComma,
    NumpadEnter,
    NumpadEquals,
    NumpadMultiply,
    NumpadSubtract,

    AbntC1,
    AbntC2,
    Apostrophe,
    Apps,
    Asterisk,
    At,
    Ax,
    Backslash,
    Calculator,
    Capital,
    Colon,
    Comma,
    Convert,
    Equals,
    Grave,
    Kana,
    Kanji,
    LAlt,
    LBracket,
    LControl,
    LShift,
    LWin,
    Mail,
    MediaSelect,
    MediaStop,
    Minus,
    Mute,
    MyComputer,
    // also called "Next"
    NavigateForward,
    // also called "Prior"
    NavigateBackward,
    NextTrack,
    NoConvert,
    OEM102,
    Period,
    PlayPause,
    Plus,
    Power,
    PrevTrack,
    RAlt,
    RBracket,
    RControl,
    RShift,
    RWin,
    Semicolon,
    Slash,
    Sleep,
    Stop,
    Sysrq,
    Tab,
    Underline,
    Unlabeled,
    VolumeDown,
    VolumeUp,
    Wake,
    WebBack,
    WebFavorites,
    WebForward,
    WebHome,
    WebRefresh,
    WebSearch,
    WebStop,
    Yen,
    Copy,
    Paste,
    Cut,
}

bitflags::bitflags! {
    /// Represents the current state of the keyboard modifiers
    ///
    /// Each flag represents a modifier and is set if this modifier is active.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
    pub struct ModifiersState: u32 {
        // left and right modifiers are currently commented out, but we should be able to support
        // them in a future release
        /// The "shift" key.
        const SHIFT = 0b100;
        // const LSHIFT = 0b010;
        // const RSHIFT = 0b001;
        /// The "control" key.
        const CTRL = 0b100 << 3;
        // const LCTRL = 0b010 << 3;
        // const RCTRL = 0b001 << 3;
        /// The "alt" key.
        const ALT = 0b100 << 6;
        // const LALT = 0b010 << 6;
        // const RALT = 0b001 << 6;
        /// This is the "windows" key on PC and "command" key on Mac.
        const LOGO = 0b100 << 9;
        // const LLOGO = 0b010 << 9;
        // const RLOGO = 0b001 << 9;
    }
}

/// The behavior of cursor grabbing.
///
/// Use this enum with [`WindowCommand::SetCursorGrab`] to grab the cursor.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum CursorGrabMode {
    /// No grabbing of the cursor is performed.
    None,

    /// The cursor is confined to the window area.
    ///
    /// There's no guarantee that the cursor will be hidden. You should hide it by yourself if you
    /// want to do so.
    ///
    /// ## Platform-specific
    ///
    /// - **macOS:** Not implemented. Always returns [`ExternalError::NotSupported`] for now.
    /// - **iOS / Android / Web:** Always returns an [`ExternalError::NotSupported`].
    Confined,

    /// The cursor is locked inside the window area to the certain position.
    ///
    /// There's no guarantee that the cursor will be hidden. You should hide it by yourself if you
    /// want to do so.
    ///
    /// ## Platform-specific
    ///
    /// - **X11 / Windows:** Not implemented. Always returns [`ExternalError::NotSupported`] for now.
    /// - **iOS / Android:** Always returns an [`ExternalError::NotSupported`].
    Locked,
}
