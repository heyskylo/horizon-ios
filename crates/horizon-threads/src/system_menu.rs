//! GLFW (`win32_window.c`, `WM_SYSCOMMAND`) drops `SC_KEYMENU` unless its `keymenu` hint is set, so
//! Alt+Space and a bare Alt never open the system menu in vanilla. Winit forwards the message to
//! `DefWindowProc`, which opens the menu and steals the keyboard (sprint on Left Alt + jump).
//! A window subclass swallows it.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{SC_KEYMENU, WM_SYSCOMMAND};

const SUBCLASS_ID: usize = 0x485a_4b4d;

/// Swallows `SC_KEYMENU` on the window at `hwnd`; call it on the thread that owns the window.
pub fn suppress(hwnd: isize) {
    // SAFETY: `hwnd` is a live window of the calling thread; `procedure` is a plain function
    // that lives as long as the process, and the id and data are unused.
    unsafe {
        SetWindowSubclass(hwnd as HWND, Some(procedure), SUBCLASS_ID, 0);
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    _data: usize,
) -> isize {
    if is_key_menu(message, wparam) {
        return 0;
    }
    // SAFETY: the arguments are the ones this procedure was called with.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// The low four bits of `wparam` are used by the system, so they are masked off.
fn is_key_menu(message: u32, wparam: usize) -> bool {
    message == WM_SYSCOMMAND && (wparam & 0xfff0) == SC_KEYMENU as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SC_CLOSE, WM_KEYDOWN};

    #[test]
    fn only_the_key_menu_command_is_swallowed() {
        assert!(is_key_menu(WM_SYSCOMMAND, SC_KEYMENU as usize));
        assert!(is_key_menu(WM_SYSCOMMAND, SC_KEYMENU as usize | 0x2));
        assert!(!is_key_menu(WM_SYSCOMMAND, SC_CLOSE as usize));
        assert!(!is_key_menu(WM_KEYDOWN, SC_KEYMENU as usize));
    }
}
