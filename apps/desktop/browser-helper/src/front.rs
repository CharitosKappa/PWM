//! The window the Windows Hello prompt comes up with (D69 §3). The browser
//! starts the helper from its service worker, with no window to give it, so
//! the helper shows a small window of its own, brings it to the front, and
//! asks Windows Hello while it's there. The browser was in front for the
//! click that asked, so it may hand the front on. Whether the prompt then
//! shows in front is checked on a real computer (`QUALITY.md`).
//! The Win32 calls are this module's `unsafe` code.
#![allow(unsafe_code)]

use std::time::Duration;

use pwm_hello::{HelloError, HelloKey, Secret};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG,
    PM_REMOVE, PeekMessageW, RegisterClassW, SetForegroundWindow, TranslateMessage, WNDCLASSW,
    WS_CAPTION, WS_EX_TOPMOST, WS_OVERLAPPED, WS_VISIBLE,
};
use windows::core::{HSTRING, w};

const TITLE: &str = "PWM: confirm with Windows Hello to unlock your browser";

/// Windows Hello with the helper's window in front for each prompt.
pub struct Fronted<H>(pub H);

impl<H: HelloKey> HelloKey for Fronted<H> {
    fn supported(&self) -> bool {
        self.0.supported()
    }

    fn create(&self, name: &str) -> Result<(), HelloError> {
        in_front(|| self.0.create(name))
    }

    fn sign(&self, name: &str, data: &[u8]) -> Result<Secret<Vec<u8>>, HelloError> {
        in_front(|| self.0.sign(name, data))
    }

    fn delete(&self, name: &str) {
        self.0.delete(name);
    }
}

/// Runs `work` (which waits for Windows Hello) on another thread while this
/// one shows the window and keeps it answering.
fn in_front<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        let window = Window::show(TITLE);
        let worker = scope.spawn(work);
        while !worker.is_finished() {
            if let Some(window) = &window {
                window.pump();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(window);
        match worker.join() {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the default handling, for a window that keeps no state.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

struct Window(HWND);

impl Window {
    /// A small window, in front; none if Windows refuses (the prompt still
    /// comes, perhaps behind).
    fn show(title: &str) -> Option<Self> {
        let class = w!("PWMBrowserHelper");
        // SAFETY: plain Win32 calls with live arguments; the class names a
        // procedure that lives as long as the program.
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class_info = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            // Zero when already registered, which is fine.
            let _ = RegisterClassW(&class_info);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                class,
                &HSTRING::from(title),
                WS_OVERLAPPED | WS_CAPTION | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                460,
                120,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            let _ = SetForegroundWindow(hwnd);
            Some(Self(hwnd))
        }
    }

    /// Handles what's waiting, so Windows never calls it "not responding".
    fn pump(&self) {
        let mut msg = MSG::default();
        // SAFETY: `msg` is a live, writable MSG for each call.
        unsafe {
            while PeekMessageW(&mut msg, Some(self.0), 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window this struct made, destroyed once.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
