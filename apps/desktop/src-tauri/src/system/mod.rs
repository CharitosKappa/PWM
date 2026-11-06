//! The system's events that lock the vault at once (D66 §6).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// Something happened that must lock the vault now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The computer is going to sleep.
    Sleeping,
    /// The session locked, signed out, or was disconnected.
    SessionLocked,
}

/// Starts watching; events arrive on `events`.
pub fn watch(events: std::sync::mpsc::Sender<Event>) {
    #[cfg(windows)]
    windows::watch(events);
    #[cfg(target_os = "linux")]
    linux::watch(events);
    #[cfg(not(any(windows, target_os = "linux")))]
    drop(events);
}

/// The wall clock jumped much further than the time that passed: the
/// computer slept (and the event was missed) or the clock was changed.
/// Either way the vault locks. `wall_gap_ms` and `mono_gap_ms` are the time
/// since the last check on each clock.
pub fn slept(wall_gap_ms: i64, mono_gap_ms: u64) -> bool {
    const SLACK_MS: i64 = 15_000;
    let mono = i64::try_from(mono_gap_ms).unwrap_or(i64::MAX);
    wall_gap_ms.saturating_sub(mono) > SLACK_MS
}

#[cfg(test)]
mod tests {
    use super::slept;

    #[test]
    fn a_wall_clock_jump_counts_as_sleep() {
        assert!(!slept(1_000, 1_000));
        assert!(!slept(10_000, 1_000));
        assert!(slept(60_000, 1_000));
        // Going back in time is not sleep.
        assert!(!slept(-60_000, 1_000));
    }
}
