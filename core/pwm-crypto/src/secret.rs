//! [`Secret<T>`]: the wrapper for every password, key and vault value.

use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

const REDACTED: &str = "[REDACTED]";

/// Holds a secret value on the heap and wipes it when dropped.
///
/// - The value is zeroized on drop.
/// - `Debug` and `Display` print `[REDACTED]`, so a secret inside a logged
///   struct never reaches logs, crash reports or test snapshots.
/// - The value lives in a `Box`, so moving a `Secret` does not leave copies of
///   it behind on the stack.
/// - There is no `Clone`, `PartialEq` or `Serialize`: copies and comparisons
///   must be explicit and go through [`expose_secret`](Self::expose_secret).
///
/// ```compile_fail,E0599
/// let a = pwm_crypto::Secret::new([0u8; 32]);
/// let b = a.clone();
/// ```
///
/// ```compile_fail,E0369
/// let a = pwm_crypto::Secret::new([0u8; 32]);
/// let b = pwm_crypto::Secret::new([0u8; 32]);
/// let _ = a == b;
/// ```
pub struct Secret<T: Zeroize>(Box<T>);

impl<T: Zeroize> Secret<T> {
    /// Wraps `value`. The caller's copy is moved in; prefer
    /// [`new_with`](Self::new_with) for values generated in place, such as keys.
    pub fn new(value: T) -> Self {
        Self(Box::new(value))
    }

    /// Creates a default value on the heap and fills it in place, so the
    /// secret never exists outside the box.
    pub fn new_with(fill: impl FnOnce(&mut T)) -> Self
    where
        T: Default,
    {
        let mut boxed = Box::<T>::default();
        fill(&mut boxed);
        Self(boxed)
    }

    /// Borrows the secret value. Keep the borrow short and never log it.
    pub fn expose_secret(&self) -> &T {
        &self.0
    }

    /// Mutably borrows the secret value.
    pub fn expose_secret_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: Zeroize> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Zeroize> Drop for Secret<T> {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl<T: Zeroize> ZeroizeOnDrop for Secret<T> {}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({REDACTED})")
    }
}

impl<T: Zeroize> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}
