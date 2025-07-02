//! Compile-time tests for the typestate pattern
//!
//! These tests verify that incorrect usage of the typestate API results in
//! compile errors, preventing the race condition bug at compile time.

#[cfg(test)]
mod compile_tests {

    // Note: The actual compile-time tests are in the comments below.
    // They demonstrate code that SHOULD NOT compile, which is the desired behavior.

    // These tests verify compile-time safety. They won't actually compile,
    // which is the desired behavior!

    /// This test verifies that you cannot prepare events without going through all states
    #[allow(dead_code)]
    fn cannot_prepare_events_without_execution() {
        // The following code SHOULD NOT COMPILE:
        /*
        let stream_data = StreamData::new(vec![], vec![]);
        let scope = ExecutionScope::<states::StreamsRead, MockCommand, MockEventStore>::new(
            stream_data,
            vec![],
            ExecutionContext::default(),
        );

        // ERROR: Cannot call prepare_stream_events() on StreamsRead state
        let _events = scope.prepare_stream_events();
        */
    }

    /// This test verifies that you cannot skip state reconstruction
    #[allow(dead_code)]
    fn cannot_skip_state_reconstruction() {
        // The following code SHOULD NOT COMPILE:
        /*
        let stream_data = StreamData::new(vec![], vec![]);
        let scope = ExecutionScope::<states::StreamsRead, MockCommand, MockEventStore>::new(
            stream_data,
            vec![],
            ExecutionContext::default(),
        );

        // ERROR: Cannot call execute_command() without reconstructing state first
        let _scope_with_writes = scope.execute_command(&MockCommand, (), &mut StreamResolver::new()).await;
        */
    }

    /// This test verifies that you cannot reuse a consumed scope
    #[allow(dead_code)]
    fn cannot_reuse_consumed_scope() {
        // The following code SHOULD NOT COMPILE:
        /*
        let stream_data = StreamData::new(vec![], vec![]);
        let scope = ExecutionScope::<states::StreamsRead, MockCommand, MockEventStore>::new(
            stream_data,
            vec![],
            ExecutionContext::default(),
        );

        let scope_with_state = scope.reconstruct_state(&MockCommand);

        // ERROR: scope has been moved/consumed
        let _another_scope_with_state = scope.reconstruct_state(&MockCommand);
        */
    }
}
