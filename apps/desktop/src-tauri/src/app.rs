//! The Tauri shell (D3 §3, D66 §6): one window showing the shared screens,
//! one command (`request`) into the backend, the tray, one copy only, and
//! the timers and system events that lock the vault.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{
    AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::Mutex;

use crate::backend::{After, Backend, Device, SystemClock};
use crate::clipboard::SystemClipboard;
use crate::http::Reqwest;
use crate::protocol::{Request, reply};
use crate::settings::SettingsFile;

#[cfg(target_os = "linux")]
type PlatformKeys = crate::keys::linux::LinuxKeys;
#[cfg(windows)]
type PlatformKeys = crate::keys::windows::DpapiKeys;

type Live = Backend<Reqwest, PlatformKeys, SystemClipboard, SystemClock>;
type Shared = Arc<Mutex<Live>>;

const MAIN: &str = "main";

/// The app's own pages: `tauri://localhost` on Linux, `http://tauri.localhost` on Windows.
fn own_page(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

/// The screens' one way into the backend.
#[tauri::command]
async fn request(
    state: tauri::State<'_, Shared>,
    request: serde_json::Value,
) -> Result<serde_json::Value, ()> {
    let Ok(request) = serde_json::from_value::<Request>(request) else {
        return Ok(reply(Err("invalid_request".to_owned())));
    };
    let shared = Arc::clone(&state);
    let (result, after) = shared.lock().await.handle(request).await;
    if after == After::Sync {
        tauri::async_runtime::spawn(async move {
            let _ = shared.lock().await.sync().await;
        });
    }
    Ok(reply(result.map_err(|f| f.0)))
}

/// Locks, and shows the screens afresh so nothing unlocked stays in the page.
async fn lock_now(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let was_unlocked = {
        let mut backend = shared.lock().await;
        let was = backend.is_unlocked();
        backend.lock();
        was
    };
    if was_unlocked {
        reload(app);
    }
}

fn reload(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.reload();
    }
}

fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The local data folder, the user's only on Linux.
fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    let dir = app.path().app_local_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

fn open_backend(dir: &Path) -> Result<Live, Box<dyn std::error::Error>> {
    let store = pwm_store::SqliteStore::open(dir.join("cache.sqlite"))?;
    let transport = Reqwest::new().map_err(|_| "http client")?;
    #[cfg(windows)]
    let keys = crate::keys::windows::DpapiKeys::new(dir.join("keys"));
    #[cfg(target_os = "linux")]
    let keys =
        tauri::async_runtime::block_on(crate::keys::linux::LinuxKeys::open(dir.join("keys")));
    Ok(Backend::new(
        store,
        transport,
        keys,
        SystemClipboard::new(),
        SystemClock::default(),
        Device::this_computer(),
        Some(SettingsFile::new(dir)),
    ))
}

fn main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let downloads = app.path().download_dir().ok();
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("PWM")
        .inner_size(420.0, 680.0)
        .min_inner_size(360.0, 480.0)
        // Left out of screenshots and screen sharing (D3 §3, as D49 on macOS).
        .content_protected(true)
        .disable_drag_drop_handler()
        .on_navigation(own_page)
        // A site link (the health report's "Change", D44 §4) opens in the
        // default browser; the app never opens a window of its own.
        .on_new_window(|url, _| {
            if matches!(url.scheme(), "https" | "http") {
                let _ = open::that_detached(url.as_str());
            }
            NewWindowResponse::Deny
        })
        // The Emergency Kit (and M7.6's exports) go to the Downloads folder.
        .on_download(move |_, event| {
            if let DownloadEvent::Requested { destination, .. } = event {
                let name = destination
                    .file_name()
                    .map(std::ffi::OsStr::to_owned)
                    .unwrap_or_else(|| "download".into());
                if let Some(dir) = &downloads {
                    *destination = free_name(dir, Path::new(&name));
                }
            }
            true
        })
        .build()
}

/// `dir/name`, or `dir/stem (2).ext` and so on: a download never replaces a file.
fn free_name(dir: &Path, name: &Path) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.file_stem().unwrap_or_default().to_string_lossy();
    let ext = name
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open PWM", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "Lock", true, None::<&str>)?;
    let at_login = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login",
        true,
        at_login,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &lock,
            &PredefinedMenuItem::separator(app)?,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("PWM")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show(app),
            "lock" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { lock_now(&app).await });
            }
            "autostart" => {
                let launch = app.autolaunch();
                let on = launch.is_enabled().unwrap_or(false);
                let _ = if on {
                    launch.disable()
                } else {
                    launch.enable()
                };
                let _ = autostart.set_checked(launch.is_enabled().unwrap_or(false));
            }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    lock_now(&app).await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// The timers, about once a second: the lock timeout, clearing the
/// clipboard, retrying a sync, and sleep missed by the system's event.
fn timers(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let clock = SystemClock::default();
        let mut last = (
            crate::backend::Clock::now_ms(&clock),
            crate::backend::Clock::mono_ms(&clock),
        );
        let mut every = tokio::time::interval(Duration::from_secs(1));
        loop {
            every.tick().await;
            let now = (
                crate::backend::Clock::now_ms(&clock),
                crate::backend::Clock::mono_ms(&clock),
            );
            let slept = crate::system::slept(now.0 - last.0, now.1.saturating_sub(last.1));
            last = now;
            if slept {
                lock_now(&app).await;
                continue;
            }
            let shared = app.state::<Shared>().inner().clone();
            let due = shared.lock().await.tick();
            if due.locked {
                reload(&app);
            }
            if due.sync {
                let _ = shared.lock().await.sync().await;
            }
        }
    });
}

/// Sleep and the session locking lock the vault at once.
fn system_events(app: AppHandle) {
    let (send, receive) = std::sync::mpsc::channel();
    crate::system::watch(send);
    std::thread::spawn(move || {
        while receive.recv().is_ok() {
            let app = app.clone();
            tauri::async_runtime::block_on(async move { lock_now(&app).await });
        }
    });
}

pub fn run() {
    let app = tauri::Builder::default()
        // First: a second launch only brings this window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![request])
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = data_dir(&handle)?;
            let backend = open_backend(&dir)?;
            app.manage::<Shared>(Arc::new(Mutex::new(backend)));
            let window = main_window(&handle)?;
            let focus = handle.clone();
            window.on_window_event(move |event| match event {
                // Leaving the window starts the lock timeout (D66 §6).
                WindowEvent::Focused(false) => {
                    let app = focus.clone();
                    tauri::async_runtime::spawn(async move {
                        app.state::<Shared>().lock().await.touch();
                    });
                }
                // Windows: closing keeps PWM in the tray. Linux: it quits.
                WindowEvent::CloseRequested { api, .. } if cfg!(windows) => {
                    api.prevent_close();
                    if let Some(window) = focus.get_webview_window(MAIN) {
                        let _ = window.hide();
                    }
                    let app = focus.clone();
                    tauri::async_runtime::spawn(async move {
                        app.state::<Shared>().lock().await.touch();
                    });
                }
                _ => {}
            });
            // Where the desktop has no tray (GNOME), the app still works.
            let _ = tray(&handle);
            timers(handle.clone());
            system_events(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the app starts");
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let shared = app.state::<Shared>().inner().clone();
            tauri::async_runtime::block_on(async move { shared.lock().await.lock() });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_never_replaces_a_file() {
        let dir = std::env::temp_dir().join(format!("pwm-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kit = Path::new("Emergency Kit.pdf");
        assert_eq!(free_name(&dir, kit), dir.join("Emergency Kit.pdf"));
        std::fs::write(dir.join("Emergency Kit.pdf"), b"x").unwrap();
        assert_eq!(free_name(&dir, kit), dir.join("Emergency Kit (2).pdf"));
        std::fs::write(dir.join("Emergency Kit (2).pdf"), b"x").unwrap();
        assert_eq!(free_name(&dir, kit), dir.join("Emergency Kit (3).pdf"));
        assert_eq!(free_name(&dir, Path::new("notes")), dir.join("notes"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_apps_own_pages_load() {
        for (url, own) in [
            ("http://tauri.localhost/index.html", true),
            ("tauri://localhost/index.html", true),
            ("https://tauri.localhost/", true),
            ("https://example.com/", false),
            ("http://tauri.localhost.example.com/", false),
            ("file:///C:/x.html", false),
        ] {
            assert_eq!(own_page(&url.parse().unwrap()), own, "{url}");
        }
    }
}
