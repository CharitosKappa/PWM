//! When the vault locks (D66 §6). Any use restarts the lock timeout, and so
//! does the window losing focus or being hidden: switching to another app
//! counts as leaving, as on the Mac (D48). Sleep, the session locking and
//! signing out of the computer lock at once (see `system`).
//!
//! This is the deadline only, over a monotonic clock in milliseconds, so it
//! is tested without waiting; `app.rs` sleeps until it and asks again.

/// The lock timeout's deadline.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AutoLock {
    deadline_ms: Option<u64>,
}

impl AutoLock {
    /// Restarts the timeout: a use, or the window losing focus.
    pub fn touch(&mut self, now_ms: u64, minutes: u32) {
        self.deadline_ms = Some(now_ms.saturating_add(u64::from(minutes) * 60_000));
    }

    /// Locked: nothing to wait for.
    pub fn clear(&mut self) {
        self.deadline_ms = None;
    }

    pub fn deadline_ms(&self) -> Option<u64> {
        self.deadline_ms
    }

    /// Whether the vault must lock now.
    pub fn due(&self, now_ms: u64) -> bool {
        self.deadline_ms.is_some_and(|d| now_ms >= d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_the_timeout_since_the_last_use() {
        let mut lock = AutoLock::default();
        assert!(!lock.due(u64::MAX));
        lock.touch(1_000, 15);
        assert!(!lock.due(1_000 + 15 * 60_000 - 1));
        assert!(lock.due(1_000 + 15 * 60_000));
        // A use restarts it.
        lock.touch(10 * 60_000, 15);
        assert!(!lock.due(1_000 + 15 * 60_000));
        assert_eq!(lock.deadline_ms(), Some(25 * 60_000));
        lock.clear();
        assert!(!lock.due(u64::MAX));
    }

    #[test]
    fn never_overflows() {
        let mut lock = AutoLock::default();
        lock.touch(u64::MAX - 5, 480);
        assert_eq!(lock.deadline_ms(), Some(u64::MAX));
    }
}
