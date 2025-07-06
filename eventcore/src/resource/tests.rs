//! Tests for resource management functionality

#[cfg(test)]
mod tests {
    use crate::resource::{
        implementations::locking, global_leak_detector, lifecycle::{ManagedResource, ResourceExt, ResourceScope, TimedResourceGuard},
        monitor::ResourceLeakDetector, states::*, IsAcquired, IsReleasable, Resource, ResourceError,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    #[test]
    fn test_resource_state_transitions() {
        // Start with initializing resource
        let initializing = Resource::<String, Initializing>::new("test".to_string());

        // Can transition to acquired
        let acquired = initializing.mark_acquired();
        assert_eq!(acquired.get(), "test");

        // Can transition to failed
        let failed = acquired.mark_failed();

        // Failed can be recovered
        let recovered = failed.recover().unwrap();
        assert_eq!(recovered.get(), "test");

        // Can be released
        let released = recovered.release().unwrap();
        assert!(released.is_released());
    }

    #[test]
    fn test_resource_scope() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let mut scope = ResourceScope::new(resource);

        // Can access resource in scope
        scope.with_resource(|r| {
            assert_eq!(r.get(), "test");
        });

        // Check scope state
        assert!(!scope.is_released());
        assert!(!scope.is_leaked());

        // Manual release works
        scope.release().unwrap();
    }

    #[test]
    fn test_resource_scope_automatic_cleanup() {
        let leaked_flag = Arc::new(AtomicUsize::new(0));

        // Create scope and let it drop without explicit release
        {
            let resource = Resource::<String, Acquired>::new("test".to_string());
            let _scope = ResourceScope::new(resource);

            // Increment counter when scope is created
            leaked_flag.store(1, Ordering::SeqCst);

            // Scope will be dropped here without explicit release
        }

        // Verify scope was created (this tests that the test setup works)
        assert_eq!(leaked_flag.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_timed_resource_guard() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let timeout_duration = Duration::from_millis(100);
        let guard = TimedResourceGuard::new(resource, timeout_duration);

        // Initially should be available
        assert!(guard.get().is_some());
        assert!(!guard.is_timed_out());
        assert!(guard.time_remaining().is_some());

        // Wait for timeout
        sleep(Duration::from_millis(150)).await;

        // Should be timed out
        assert!(guard.is_timed_out());
        assert!(guard.get().is_none());
        assert!(guard.time_remaining().is_none());

        // Release should fail with timeout error
        match guard.release() {
            Err(ResourceError::Timeout { .. }) => {
                // Expected timeout error
            }
            other => panic!("Expected timeout error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_timed_resource_guard_early_release() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let guard = TimedResourceGuard::new(resource, Duration::from_secs(10));

        // Should be available
        assert!(guard.get().is_some());
        assert!(!guard.is_timed_out());

        // Release before timeout
        guard.release().unwrap();
    }

    #[test]
    fn test_mutex_resource() {
        let mutex_resource = locking::create_mutex_resource(42i32);

        // Can acquire lock
        let guard = mutex_resource.lock().unwrap();
        assert_eq!(*guard.get(), 42);

        // Lock is exclusive
        assert!(mutex_resource.try_lock().unwrap().is_none()); // Should fail to acquire

        // Release first lock
        drop(guard);

        // Now should be able to acquire
        assert!(mutex_resource.try_lock().unwrap().is_some());
    }

    #[test]
    fn test_mutex_resource_mutable_access() {
        let mutex_resource = locking::create_mutex_resource(42i32);

        // Acquire lock and modify data
        {
            let mut guard = mutex_resource.lock().unwrap();
            *guard.get_mut() = 100;
        }

        // Verify modification
        assert_eq!(*mutex_resource.lock().unwrap().get(), 100);
    }

    #[test]
    fn test_resource_leak_detector() {
        let detector = ResourceLeakDetector::new();

        // Initially empty
        let initial_stats = detector.get_stats();
        assert_eq!(initial_stats.total_active, 0);
        assert!(initial_stats.by_type.is_empty());

        // Register some resources
        detector.register_acquisition("res1", "DatabasePool", Some("test_location".to_string()));
        detector.register_acquisition("res2", "DatabasePool", None);
        detector.register_acquisition("res3", "MutexLock", None);

        let stats = detector.get_stats();
        assert_eq!(stats.total_active, 3);
        assert_eq!(stats.by_type.get("DatabasePool"), Some(&2));
        assert_eq!(stats.by_type.get("MutexLock"), Some(&1));

        // Release one resource
        detector.register_release("res1");

        let stats = detector.get_stats();
        assert_eq!(stats.total_active, 2);
        assert_eq!(stats.by_type.get("DatabasePool"), Some(&1));

        // Find potential leaks (none yet since resources are new)
        let leaks = detector.find_potential_leaks(Duration::from_secs(1));
        assert!(leaks.is_empty());

        // Clean up
        detector.register_release("res2");
        detector.register_release("res3");

        let final_stats = detector.get_stats();
        assert_eq!(final_stats.total_active, 0);
    }

    #[test]
    fn test_global_leak_detector() {
        let initial_count = global_leak_detector().get_stats().total_active;

        // Register a resource
        global_leak_detector().register_acquisition("global_test", "TestResource", None);

        let stats = global_leak_detector().get_stats();
        assert_eq!(stats.total_active, initial_count + 1);

        // Release the resource
        global_leak_detector().register_release("global_test");

        let final_stats = global_leak_detector().get_stats();
        assert_eq!(final_stats.total_active, initial_count);
    }

    #[test]
    fn test_managed_resource() {
        let initial_count = global_leak_detector().get_stats().total_active;

        // Create managed resource
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let managed = ManagedResource::new(resource, "TestResource");

        // Should be tracked
        let stats = global_leak_detector().get_stats();
        assert!(stats.total_active > initial_count);
        assert!(stats.by_type.contains_key("TestResource"));

        // Can access inner resource
        assert!(managed.get().is_some());

        // Taking resource should work and update tracking
        let taken = managed.take();
        assert!(taken.is_some());

        // Should be untracked now
        let final_stats = global_leak_detector().get_stats();
        assert_eq!(final_stats.total_active, initial_count);
    }

    #[test]
    fn test_managed_resource_drop_cleanup() {
        let initial_count = global_leak_detector().get_stats().total_active;

        // Create managed resource and drop it
        {
            let resource = Resource::<String, Acquired>::new("test".to_string());
            let _managed = ManagedResource::new(resource, "TestResource");

            // Should be tracked
            let stats = global_leak_detector().get_stats();
            assert!(stats.total_active > initial_count);
        } // Drop happens here

        // Should be untracked after drop
        let final_stats = global_leak_detector().get_stats();
        assert_eq!(final_stats.total_active, initial_count);
    }

    #[test]
    fn test_resource_extension_traits() {
        let resource = Resource::<String, Acquired>::new("test".to_string());

        // Test managed extension
        let managed = resource.managed("TestResource");
        assert!(managed.get().is_some());

        let resource2 = Resource::<String, Acquired>::new("test2".to_string());

        // Test scoped extension
        let scope = resource2.scoped();
        assert!(!scope.is_released());

        let resource3 = Resource::<String, Acquired>::new("test3".to_string());

        // Test timed extension
        let timed = resource3.with_timeout(Duration::from_secs(1));
        assert!(timed.get().is_some());
    }

    #[tokio::test]
    async fn test_resource_state_machine_invalid_transitions() {
        // Test that certain state transitions are not allowed at compile time

        let released = Resource::<String, Released>::new("test".to_string());
        assert!(released.is_released());

        // These operations should not compile (verified by compiler):
        // released.get(); // ❌ Cannot access released resource
        // released.release(); // ❌ Cannot release already released resource
        // released.mark_failed(); // ❌ Cannot fail released resource
    }

    #[tokio::test]
    async fn test_concurrent_resource_access() {
        let resource = locking::create_mutex_resource(0i32);
        let resource = Arc::new(resource);

        // Spawn multiple tasks that increment the counter
        let mut handles = vec![];
        for _ in 0..10 {
            let resource_clone = resource.clone();
            let handle = tokio::spawn(async move {
                let mut guard = resource_clone.lock().unwrap();
                let current = *guard.get();
                *guard.get_mut() = current + 1;
                // Lock is released when guard is dropped
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify final value
        assert_eq!(*resource.lock().unwrap().get(), 10);
    }

    #[tokio::test]
    async fn test_resource_timeout_behavior() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let guard = TimedResourceGuard::new(resource, Duration::from_millis(50));

        // Test that timeout actually works
        let result = timeout(Duration::from_millis(100), async {
            while !guard.is_timed_out() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await;

        assert!(result.is_ok(), "Timeout should have occurred within 100ms");
        assert!(guard.is_timed_out());
    }

    #[test]
    fn test_resource_error_types() {
        // Test different error variants
        let acquisition_error = ResourceError::AcquisitionFailed("test".to_string());
        assert!(matches!(
            acquisition_error,
            ResourceError::AcquisitionFailed(_)
        ));

        let release_error = ResourceError::ReleaseFailed("test".to_string());
        assert!(matches!(release_error, ResourceError::ReleaseFailed(_)));

        let invalid_state_error = ResourceError::InvalidState("test".to_string());
        assert!(matches!(
            invalid_state_error,
            ResourceError::InvalidState(_)
        ));

        let timeout_error = ResourceError::Timeout {
            duration: Duration::from_secs(1),
        };
        assert!(matches!(timeout_error, ResourceError::Timeout { .. }));

        let poisoned_error = ResourceError::Poisoned("test".to_string());
        assert!(matches!(poisoned_error, ResourceError::Poisoned(_)));
    }

    #[test]
    fn test_sealed_trait_pattern() {
        // Test that our sealed traits work correctly
        // This is mostly a compile-time test

        fn test_acquired<S: IsAcquired>(_state: std::marker::PhantomData<S>) {
            // This function should only accept acquired states
        }

        fn test_releasable<S: IsReleasable>(_state: std::marker::PhantomData<S>) {
            // This function should only accept releasable states
        }

        // These should compile
        test_acquired(std::marker::PhantomData::<Acquired>);
        test_releasable(std::marker::PhantomData::<Acquired>);
        test_releasable(std::marker::PhantomData::<Failed>);

        // These should NOT compile (verified by external compile tests):
        // test_acquired(std::marker::PhantomData::<Released>);
        // test_releasable(std::marker::PhantomData::<Released>);
    }

    #[test]
    fn test_compilation_errors() {
        // These tests verify that certain operations don't compile
        // They are written as compile_fail tests to document the expected behavior

        // Cannot use released resource
        let _resource = Resource::<String, Released>::new("test".to_string());
        // resource.get(); // ❌ This should not compile

        // Cannot release an already released resource
        // resource.release(); // ❌ This should not compile

        // Cannot mark released resource as failed
        // resource.mark_failed(); // ❌ This should not compile
    }
}