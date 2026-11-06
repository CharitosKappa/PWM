//! Copying (D66 §7): through the backend, never the webview, and cleared
//! 30 seconds later whatever the clipboard holds then (D34). On Windows the
//! copy is left out of the clipboard history and the cloud clipboard; on
//! Linux it carries the password-manager hint clipboard managers skip.

use std::sync::Mutex;

/// The clipboard is cleared this long after a copy.
pub const CLEAR_AFTER_MS: u64 = 30_000;

/// The clipboard refused the copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardError;

pub trait Clipboard: Send + Sync + 'static {
    fn write(&self, text: &str) -> Result<(), ClipboardError>;
    fn clear(&self) -> Result<(), ClipboardError>;
}

/// The system clipboard. One instance stays open: on Linux the copy lives
/// as long as it does. Without a clipboard (no display), every copy fails
/// and says so; the app still opens.
pub struct SystemClipboard {
    inner: Option<Mutex<arboard::Clipboard>>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            inner: arboard::Clipboard::new().ok().map(Mutex::new),
        }
    }

    fn get(&self) -> Result<std::sync::MutexGuard<'_, arboard::Clipboard>, ClipboardError> {
        self.inner
            .as_ref()
            .ok_or(ClipboardError)?
            .lock()
            .map_err(|_| ClipboardError)
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for SystemClipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        let mut clipboard = self.get()?;
        let set = clipboard.set();
        #[cfg(windows)]
        let set = {
            use arboard::SetExtWindows;
            set.exclude_from_history().exclude_from_cloud()
        };
        #[cfg(target_os = "linux")]
        let set = {
            use arboard::SetExtLinux;
            set.exclude_from_history()
        };
        set.text(text.to_owned()).map_err(|_| ClipboardError)
    }

    fn clear(&self) -> Result<(), ClipboardError> {
        let mut clipboard = self.get()?;
        clipboard.clear().map_err(|_| ClipboardError)
    }
}

/// What was copied, for tests.
#[derive(Default)]
pub struct MemoryClipboard {
    pub text: Mutex<Option<String>>,
}

impl Clipboard for MemoryClipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        *self.text.lock().map_err(|_| ClipboardError)? = Some(text.to_owned());
        Ok(())
    }

    fn clear(&self) -> Result<(), ClipboardError> {
        *self.text.lock().map_err(|_| ClipboardError)? = None;
        Ok(())
    }
}
