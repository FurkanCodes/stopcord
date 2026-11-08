//! A key that takes you out of a terminal and back to Sawtooth. Inside a terminal every
//! plain key belongs to the agent, and Esc stops its turn, so the way out is a ⌘ key,
//! which a terminal never receives: ⌘↑, the Mac's "up a level", or ⌘esc where the
//! system doesn't take it first. The web view keeps some ⌘ keys for itself, so on macOS
//! the app watches for them before the page does, and only while a terminal has the keys.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

/// The event the page listens for to leave a terminal.
pub const EVENT: &str = "leave-terminal";

/// True while a terminal has the keys, so ⌘↑ keeps its usual meaning everywhere else.
static IN_TERMINAL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn in_terminal(on: bool) {
    IN_TERMINAL.store(on, Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
pub fn watch(app: &AppHandle) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};

    const ESCAPE: u16 = 53;
    const UP: u16 = 126;
    let app = app.clone();
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the monitor a live event for the length of the call.
        let e = unsafe { event.as_ref() };
        let mods = e.modifierFlags()
            & NSEventModifierFlags::DeviceIndependentFlagsMask
            & !NSEventModifierFlags::NumericPad
            & !NSEventModifierFlags::Function;
        let leave = matches!(e.keyCode(), ESCAPE | UP) && mods == NSEventModifierFlags::Command;
        if leave && IN_TERMINAL.load(Ordering::Relaxed) {
            let _ = app.emit(EVENT, ());
            return std::ptr::null_mut();
        }
        event.as_ptr()
    });
    // SAFETY: called on the main thread during setup; the handler returns the event or null.
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler) };
    // The monitor lives as long as the app.
    std::mem::forget(monitor);
}

#[cfg(not(target_os = "macos"))]
pub fn watch(_app: &AppHandle) {}
