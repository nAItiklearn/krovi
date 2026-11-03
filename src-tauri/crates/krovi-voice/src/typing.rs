//! Type dictated text into whatever window has focus — without touching the clipboard.
//!
//! Each UTF-16 unit becomes a `KEYEVENTF_UNICODE` key down/up pair sent with `SendInput`;
//! `\n` is sent as Enter and `\t` as Tab. Nothing is copied or pasted, so the user's
//! clipboard is never read or overwritten.
//!
//! Honest failure: Windows silently drops synthetic input aimed at a window running with
//! higher privileges (UIPI) — `SendInput` then reports fewer events than requested and we
//! return [`TypeError::Blocked`] instead of pretending the text arrived. If the user is
//! still holding a modifier (the dictation shortcut), typing waits for its release so the
//! characters cannot turn into shortcuts; if it is never released we refuse.

use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN,
    VK_RWIN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::typing_plan::{plan, Stroke};

/// Inputs per `SendInput` call (a key down + up per character).
const BATCH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    /// No window has keyboard focus.
    NoTarget,
    /// A modifier key stayed held, so typing would have triggered shortcuts.
    ModifiersHeld,
    /// Windows dropped the input (the target runs as administrator, or a secure desktop
    /// such as the lock screen or a UAC prompt is in front). `typed` characters got through.
    Blocked { typed: usize },
    /// The focused window changed mid-way; we stopped to avoid typing into the wrong app.
    FocusChanged { typed: usize },
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::NoTarget => f.write_str("No text field has focus."),
            TypeError::ModifiersHeld => {
                f.write_str("Release Ctrl / Alt / Shift / Win so the text can be typed.")
            }
            TypeError::Blocked { .. } => f.write_str(
                "Windows blocked typing into that window (it may be running as administrator).",
            ),
            TypeError::FocusChanged { .. } => {
                f.write_str("The focused window changed, so typing stopped.")
            }
        }
    }
}

impl std::error::Error for TypeError {}

fn modifiers_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|k| unsafe { GetAsyncKeyState(i32::from(k.0)) } < 0)
}

/// Wait up to `max` for every modifier key to be released.
pub fn wait_for_modifiers_released(max: Duration) -> bool {
    let start = Instant::now();
    while modifiers_down() {
        if start.elapsed() > max {
            return false;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    true
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn inputs_for(stroke: Stroke) -> [INPUT; 2] {
    match stroke {
        Stroke::Unit(u) => [
            key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE),
            key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
        ],
        Stroke::Enter => [
            key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)),
            key(VK_RETURN, 0, KEYEVENTF_KEYUP),
        ],
        Stroke::Tab => [
            key(VK_TAB, 0, KEYBD_EVENT_FLAGS(0)),
            key(VK_TAB, 0, KEYEVENTF_KEYUP),
        ],
    }
}

/// Type `text` into the focused window. Returns the number of keystrokes typed.
pub fn type_text(text: &str) -> Result<usize, TypeError> {
    if !wait_for_modifiers_released(Duration::from_millis(1500)) {
        return Err(TypeError::ModifiersHeld);
    }
    let target = unsafe { GetForegroundWindow() };
    if target.0.is_null() {
        return Err(TypeError::NoTarget);
    }
    let strokes = plan(text);
    let mut typed = 0usize;
    for chunk in strokes.chunks(BATCH / 2) {
        if unsafe { GetForegroundWindow() } != target {
            return Err(TypeError::FocusChanged { typed });
        }
        let inputs: Vec<INPUT> = chunk.iter().flat_map(|&s| inputs_for(s)).collect();
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
        typed += sent / 2;
        if sent < inputs.len() {
            return Err(TypeError::Blocked { typed });
        }
        // give the target's message loop a moment on long dictations
        if strokes.len() > BATCH {
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    Ok(typed)
}
