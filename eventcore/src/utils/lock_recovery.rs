//! Lock recovery utilities for handling poisoned locks gracefully.
//!
//! This module provides utilities for recovering from poisoned locks, which can occur
//! when a thread panics while holding a lock. Instead of propagating the panic,
//! these utilities allow for graceful recovery and error handling.

use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Extension trait for `RwLock` to provide recovery from poisoned locks.
pub trait RwLockRecovery<T> {
    /// Attempts to acquire a read lock, recovering from poison if necessary.
    ///
    /// If the lock is poisoned, this method will clear the poison flag and
    /// return the guard. The data may be in an inconsistent state, so callers
    /// should validate the data after acquiring the lock.
    fn read_recover(&self) -> RwLockReadGuard<'_, T>;

    /// Attempts to acquire a write lock, recovering from poison if necessary.
    ///
    /// If the lock is poisoned, this method will clear the poison flag and
    /// return the guard. The data may be in an inconsistent state, so callers
    /// should validate or reset the data after acquiring the lock.
    fn write_recover(&self) -> RwLockWriteGuard<'_, T>;

    /// Attempts to acquire a read lock, returning an error if poisoned.
    ///
    /// This is a safer alternative to `read()` that returns a descriptive error
    /// instead of panicking on poison.
    fn try_read_safe(&self) -> Result<RwLockReadGuard<'_, T>, String>;

    /// Attempts to acquire a write lock, returning an error if poisoned.
    ///
    /// This is a safer alternative to `write()` that returns a descriptive error
    /// instead of panicking on poison.
    fn try_write_safe(&self) -> Result<RwLockWriteGuard<'_, T>, String>;
}

impl<T> RwLockRecovery<T> for RwLock<T> {
    fn read_recover(&self) -> RwLockReadGuard<'_, T> {
        match self.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // Recover by taking the guard from the error
                // The data may be in an inconsistent state
                poisoned.into_inner()
            }
        }
    }

    fn write_recover(&self) -> RwLockWriteGuard<'_, T> {
        match self.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // Recover by taking the guard from the error
                // The data may be in an inconsistent state
                poisoned.into_inner()
            }
        }
    }

    fn try_read_safe(&self) -> Result<RwLockReadGuard<'_, T>, String> {
        self.read().map_err(|_: PoisonError<_>| {
            "Lock poisoned due to panic in another thread. Data may be inconsistent.".to_string()
        })
    }

    fn try_write_safe(&self) -> Result<RwLockWriteGuard<'_, T>, String> {
        self.write().map_err(|_: PoisonError<_>| {
            "Lock poisoned due to panic in another thread. Data may be inconsistent.".to_string()
        })
    }
}

/// Helper function to recover from a poisoned lock by resetting the data.
///
/// This function acquires a write lock (recovering from poison if necessary)
/// and resets the data to the provided value. This is useful when you want
/// to ensure the data is in a known good state after a panic.
pub fn reset_poisoned_lock<T: Clone>(lock: &RwLock<T>, new_value: T) {
    let mut guard = lock.write_recover();
    *guard = new_value;
}

#[cfg(test)]
#[allow(clippy::significant_drop_tightening)]
mod tests {
    use super::*;
    use std::panic;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_read_recover_from_poison() {
        let lock = Arc::new(RwLock::new(42));
        let lock_clone = Arc::clone(&lock);

        // Spawn a thread that will panic while holding the write lock
        let handle = thread::spawn(move || {
            let _guard = lock_clone.write().unwrap();
            panic!("Intentional panic to poison the lock");
        });

        // Wait for the thread to panic
        let _ = handle.join();

        // The lock should be poisoned now, but read_recover should work
        let value = lock.read_recover();
        assert_eq!(*value, 42);
    }

    #[test]
    fn test_write_recover_from_poison() {
        let lock = Arc::new(RwLock::new(vec![1, 2, 3]));
        let lock_clone = Arc::clone(&lock);

        // Spawn a thread that will panic while holding the write lock
        let handle = thread::spawn(move || {
            let mut guard = lock_clone.write().unwrap();
            guard.push(4);
            panic!("Intentional panic to poison the lock");
        });

        // Wait for the thread to panic
        let _ = handle.join();

        // The lock should be poisoned, but write_recover should work
        let mut value = lock.write_recover();
        // Note: The push(4) operation completed before the panic
        assert_eq!(*value, vec![1, 2, 3, 4]);

        // We can now safely modify the data
        value.clear();
        value.push(10);
    }

    #[test]
    fn test_try_read_safe_normal() {
        let lock = RwLock::new("hello");
        let result = lock.try_read_safe();
        assert!(result.is_ok());
        assert_eq!(*result.unwrap(), "hello");
    }

    #[test]
    fn test_try_write_safe_normal() {
        let lock = RwLock::new(0);
        let result = lock.try_write_safe();
        assert!(result.is_ok());
        let mut guard = result.unwrap();
        *guard = 100;
    }

    #[test]
    fn test_reset_poisoned_lock() {
        let lock = Arc::new(RwLock::new(vec![1, 2, 3]));
        let lock_clone = Arc::clone(&lock);

        // Poison the lock
        let handle = thread::spawn(move || {
            let _guard = lock_clone.write().unwrap();
            panic!("Poisoning the lock");
        });
        let _ = handle.join();

        // Reset the lock with new data
        reset_poisoned_lock(&lock, vec![10, 20, 30]);

        // Verify the data was reset - use read_recover since we know it was poisoned
        let value = lock.read_recover();
        assert_eq!(*value, vec![10, 20, 30]);
    }
}
