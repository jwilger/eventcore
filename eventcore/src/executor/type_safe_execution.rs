use crate::command::{Command, CommandResult, StreamResolver};
use crate::errors::CommandError;
use crate::event_store::{EventStore, ReadOptions};
use crate::executor::{typestate, CommandExecutor, ExecutionOptions};
use crate::types::{EventVersion, StreamId};
use std::collections::HashMap;
use tracing::{info, instrument};

impl<ES> CommandExecutor<ES>
where
    ES: EventStore,
{
    /// Execute a command using type-safe execution scope.
    ///
    /// This method uses the typestate pattern to ensure that the same `StreamData`
    /// is used for both state reconstruction and event writing, making race conditions
    /// impossible at compile time.
    ///
    /// # Type Parameters
    ///
    /// * `C` - The command type to execute
    ///
    /// # Arguments
    ///
    /// * `command` - The command instance
    /// * `options` - Execution options
    ///
    /// # Returns
    ///
    /// A result containing a map of stream IDs to their new versions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = executor.execute_type_safe(&command, ExecutionOptions::default()).await?;
    /// ```
    #[instrument(
        skip(self, command, options),
        fields(
            command_type = std::any::type_name::<C>(),
            correlation_id = %options.context.correlation_id,
            user_id = options.context.user_id.as_deref().unwrap_or("anonymous"),
            type_safe_execution = true
        )
    )]
    pub async fn execute_type_safe<C>(
        &self,
        command: C,
        options: ExecutionOptions,
    ) -> CommandResult<HashMap<StreamId, EventVersion>>
    where
        C: Command,
        C::Event: Clone
            + PartialEq
            + Eq
            + for<'a> TryFrom<&'a ES::Event>
            + serde::Serialize
            + Send
            + Sync,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
        ES::Event: From<C::Event> + Clone + serde::Serialize,
    {
        let mut stream_ids = command.read_streams();
        let mut stream_resolver = StreamResolver::new();
        let mut iteration = 0;
        let max_iterations = options.max_stream_discovery_iterations;

        loop {
            iteration += 1;
            if iteration > max_iterations {
                return Err(CommandError::ValidationFailed(format!(
                    "Command '{}' exceeded maximum stream discovery iterations ({max_iterations})",
                    std::any::type_name::<C>()
                )));
            }

            info!(
                iteration,
                streams_count = stream_ids.len(),
                "Starting stream discovery iteration"
            );

            // Read streams and create type-safe execution scope
            let stream_data = self
                .read_streams_with_circuit_breaker(&stream_ids, &ReadOptions::new(), &options)
                .await
                .map_err(CommandError::from)?;

            // Create type-safe execution scope
            let scope = typestate::ExecutionScope::<typestate::states::StreamsRead, C, ES>::new(
                stream_data,
                stream_ids.clone(),
                options.context.clone(),
            );

            // Reconstruct state using the scope
            let scope_with_state = scope.reconstruct_state(&command);

            // Check if we need additional streams
            let additional_streams = scope_with_state.check_additional_streams(&stream_resolver);
            if !additional_streams.is_empty() {
                info!(
                    new_streams_count = additional_streams.len(),
                    "Command requested additional streams, re-reading"
                );
                stream_ids.extend(additional_streams);
                continue;
            }

            // Execute command
            let scope_with_writes = scope_with_state
                .execute_command(&command, &mut stream_resolver)
                .await?;

            // Check again for additional streams after execution
            let new_additional = stream_resolver
                .take_additional_streams()
                .into_iter()
                .filter(|s| !stream_ids.contains(s))
                .collect::<Vec<_>>();

            if !new_additional.is_empty() {
                info!(
                    new_streams_count = new_additional.len(),
                    "Command requested additional streams after execution, re-reading"
                );
                stream_ids.extend(new_additional);
                continue;
            }

            // Prepare stream events using the type-safe scope
            let stream_events = scope_with_writes.prepare_stream_events();

            // Write events
            let result_versions = self
                .write_events_with_circuit_breaker(&stream_events, &options)
                .await
                .map_err(CommandError::from)?;

            info!(
                written_streams = result_versions.len(),
                "Command execution completed successfully"
            );

            return Ok(result_versions);
        }
    }
}
