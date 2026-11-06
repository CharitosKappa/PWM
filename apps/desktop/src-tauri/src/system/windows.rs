//! Windows: the session locking, signing out or disconnecting (through a
//! message-only window registered for session notifications) and the
//! computer going to sleep (a suspend callback) lock the vault at once
//! (D66 §6). Only the Win32 calls are `unsafe`; see `keys::windows`.
#![allow(unsafe_code)]

use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, PowerRegisterSuspendResumeNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_CALLBACK, DefWindowProcW, DispatchMessageW, GetMessageW,
    HWND_MESSAGE, MSG, PBT_APMSUSPEND, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_DISCONNECT,
    WTS_SESSION_LOCK, WTS_SESSION_LOGOFF,
};
use windows::core::w;

use super::Event;

/// Where the callbacks send what happened; set once.
static EVENTS: OnceLock<Sender<Event>> = OnceLock::new();

fn send(event: Event) {
    if let Some(events) = EVENTS.get() {
        let _ = events.send(event);
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_WTSSESSION_CHANGE {
        let change = u32::try_from(wparam.0).unwrap_or(0);
        if matches!(
            change,
            WTS_SESSION_LOCK | WTS_SESSION_LOGOFF | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT
        ) {
            send(Event::SessionLocked);
        }
        return LRESULT(0);
    }
    // SAFETY: the default handling for every other message of our window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn on_power(
    _context: *const core::ffi::c_void,
    kind: u32,
    _setting: *const core::ffi::c_void,
) -> u32 {
    if kind == PBT_APMSUSPEND {
        send(Event::Sleeping);
    }
    0
}

/// Starts watching; events arrive on `events` from a thread of its own.
pub fn watch(events: Sender<Event>) {
    if EVENTS.set(events).is_err() {
        return;
    }
    std::thread::spawn(|| {
        // SAFETY: a message-only window of our own class on this thread,
        // with its message loop below; the power callback is a plain
        // function that only sends on the channel.
        unsafe {
            let Ok(instance) = GetModuleHandleW(None) else {
                return;
            };
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("PwmSessionEvents"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return;
            }
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("PwmSessionEvents"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            ) else {
                return;
            };
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

            // Kept for the life of the process, which the registration is for.
            let params: &'static DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS =
                Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                    Callback: Some(on_power),
                    Context: std::ptr::null_mut(),
                }));
            let mut registration: *mut core::ffi::c_void = std::ptr::null_mut();
            let _ = PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(std::ptr::from_ref(params).cast_mut().cast()),
                &mut registration,
            );

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
    });
}
