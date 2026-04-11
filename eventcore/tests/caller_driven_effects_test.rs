//! Integration test for caller-driven effects (Issue #299).
//!
//! Commands that need external I/O during execution declare Effect and
//! EffectResult associated types. The `execute()` function returns an
//! `Execution` enum that may yield effects back to the caller, who
//! fulfills them and resumes execution.

use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, Execution, HandleDecision, RetryPolicy,
    StreamDeclarations, StreamId,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- Domain types ---

fn test_stream_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TokenEvent {
    TokenValidated {
        stream_id: StreamId,
        token: String,
        user_id: String,
    },
}

impl Event for TokenEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            TokenEvent::TokenValidated { stream_id, .. } => stream_id,
        }
    }

    fn event_type_name() -> &'static str {
        "TokenEvent"
    }
}

// --- Effect types (application-defined) ---

/// What the command can ask the caller to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthEffect {
    ValidateToken { token: String },
}

/// What the caller returns after fulfilling the effect.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthEffectResult {
    TokenValid { user_id: String },
    TokenInvalid,
}

// --- Command ---

struct ValidateToken {
    stream_id: StreamId,
    token: String,
}

impl CommandStreams for ValidateToken {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![self.stream_id.clone()]).expect("single stream")
    }
}

impl CommandLogic for ValidateToken {
    type Event = TokenEvent;
    type State = ();
    type Effect = AuthEffect;
    type EffectResult = AuthEffectResult;

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> HandleDecision<Self> {
        // Request the caller to validate the token
        HandleDecision::Effect(AuthEffect::ValidateToken {
            token: self.token.clone(),
        })
    }

    fn resume(&self, _state: Self::State, result: Self::EffectResult) -> HandleDecision<Self> {
        match result {
            AuthEffectResult::TokenValid { user_id } => {
                HandleDecision::Done(Ok(vec![TokenEvent::TokenValidated {
                    stream_id: self.stream_id.clone(),
                    token: self.token.clone(),
                    user_id,
                }]
                .into()))
            }
            AuthEffectResult::TokenInvalid => HandleDecision::Done(Err(
                CommandError::BusinessRuleViolation("invalid-token".to_string()),
            )),
        }
    }
}

// --- Tests ---

/// Given a command that requires an external effect (token validation),
/// When execute() is called and the caller fulfills the effect,
/// Then the command completes successfully with the validated result.
#[tokio::test]
async fn effect_command_success_after_fulfillment() {
    // Given: an event store and a command that needs token validation
    let store = InMemoryEventStore::new();
    let stream_id = test_stream_id();
    let command = ValidateToken {
        stream_id: stream_id.clone(),
        token: "valid-jwt-token".to_string(),
    };

    // When: execute returns an effect request
    let mut result = eventcore::execute(&store, command, RetryPolicy::default()).await;

    // Then: the result is an effect requesting token validation
    let effect_request = match result {
        Execution::Effect(req) => req,
        other => panic!("expected Effect, got {:?}", other),
    };
    assert_eq!(
        *effect_request.effect(),
        AuthEffect::ValidateToken {
            token: "valid-jwt-token".to_string()
        }
    );

    // When: the caller fulfills the effect with a valid result
    result = effect_request
        .resume(AuthEffectResult::TokenValid {
            user_id: "user-123".to_string(),
        })
        .await;

    // Then: execution completes successfully
    match result {
        Execution::Success(_response) => {} // success
        other => panic!("expected Success, got {:?}", other),
    }
}

/// Given a command that requires an external effect,
/// When the caller returns an invalid result,
/// Then the command fails with a business rule violation.
#[tokio::test]
async fn effect_command_failure_from_effect_result() {
    // Given: an event store and a command that needs token validation
    let store = InMemoryEventStore::new();
    let stream_id = test_stream_id();
    let command = ValidateToken {
        stream_id: stream_id.clone(),
        token: "expired-token".to_string(),
    };

    // When: execute returns an effect request
    let result = eventcore::execute(&store, command, RetryPolicy::default()).await;
    let effect_request = match result {
        Execution::Effect(req) => req,
        other => panic!("expected Effect, got {:?}", other),
    };

    // When: the caller returns an invalid token result
    let result = effect_request.resume(AuthEffectResult::TokenInvalid).await;

    // Then: execution fails with a business rule violation
    match result {
        Execution::Error(CommandError::BusinessRuleViolation(msg)) => {
            assert_eq!(msg, "invalid-token");
        }
        other => panic!("expected Error(BusinessRuleViolation), got {:?}", other),
    }
}

/// Given a command with Effect = () (no effects),
/// When execute() is called,
/// Then it returns Success or Error directly (backward compatible).
#[tokio::test]
async fn effect_free_command_returns_success_directly() {
    // Given: a simple deposit command with no effects
    let store = InMemoryEventStore::new();
    let stream_id = test_stream_id();

    // A minimal effect-free command
    struct SimpleDeposit {
        stream_id: StreamId,
    }

    impl CommandStreams for SimpleDeposit {
        fn stream_declarations(&self) -> StreamDeclarations {
            StreamDeclarations::try_from_streams(vec![self.stream_id.clone()])
                .expect("single stream")
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    enum SimpleEvent {
        Deposited { stream_id: StreamId },
    }

    impl Event for SimpleEvent {
        fn stream_id(&self) -> &StreamId {
            match self {
                SimpleEvent::Deposited { stream_id } => stream_id,
            }
        }
        fn event_type_name() -> &'static str {
            "SimpleEvent"
        }
    }

    impl CommandLogic for SimpleDeposit {
        type Event = SimpleEvent;
        type State = ();
        type Effect = ();
        type EffectResult = ();

        fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
            state
        }

        fn handle(&self, _state: Self::State) -> HandleDecision<Self> {
            HandleDecision::Done(Ok(vec![SimpleEvent::Deposited {
                stream_id: self.stream_id.clone(),
            }]
            .into()))
        }

        fn resume(&self, _state: Self::State, _result: Self::EffectResult) -> HandleDecision<Self> {
            unreachable!("effect-free commands never resume")
        }
    }

    let command = SimpleDeposit {
        stream_id: stream_id.clone(),
    };

    // When: execute is called
    let result = eventcore::execute(store, command, RetryPolicy::default()).await;

    // Then: it returns Success directly (no Effect variant)
    match result {
        Execution::Success(_response) => {} // success
        other => panic!("expected Success, got {:?}", other),
    }
}
