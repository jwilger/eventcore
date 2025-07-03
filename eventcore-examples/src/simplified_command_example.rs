//! Example demonstrating the simplified Command derive macro
//!
//! Shows how #[derive(Command)] now automatically generates:
//! - type StreamSet = CommandNameStreamSet;
//! - fn read_streams() implementation
//! - StreamSet type definition
//!
//! Commands are now always their own input - no separate Input type needed

use async_trait::async_trait;
use eventcore::{prelude::*, CommandLogic, ReadStreams, StreamWrite};
use eventcore_macros::Command;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BankingEvent {
    MoneyTransferred {
        from: String,
        to: String,
        amount: u64,
    },
}

impl TryFrom<&BankingEvent> for BankingEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &BankingEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[derive(Default)]
pub struct AccountBalances {
    balances: std::collections::HashMap<String, u64>,
}

impl AccountBalances {
    pub fn balance(&self, stream_id: &StreamId) -> u64 {
        self.balances.get(stream_id.as_ref()).copied().unwrap_or(0)
    }

    pub fn debit(&mut self, stream_id: &StreamId, amount: u64) {
        let balance = self.balance(stream_id);
        self.balances.insert(
            stream_id.as_ref().to_string(),
            balance.saturating_sub(amount),
        );
    }

    pub fn credit(&mut self, stream_id: &StreamId, amount: u64) {
        let balance = self.balance(stream_id);
        self.balances
            .insert(stream_id.as_ref().to_string(), balance + amount);
    }
}

// ✨ BEFORE: Manual boilerplate (old way)
// #[derive(Command)]
// struct TransferMoneyOld {
//     #[stream]
//     from_account: StreamId,
//     #[stream]
//     to_account: StreamId,
//     amount: u64,
// }
//
// #[async_trait]
// impl Command for TransferMoneyOld {
//     type Input = Self;                           // ❌ Manual
//     type StreamSet = TransferMoneyOldStreamSet;  // ❌ Manual
//     type State = AccountBalances;
//     type Event = BankingEvent;
//
//     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {  // ❌ Manual
//         vec![input.from_account.clone(), input.to_account.clone()]
//     }
//
//     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) { ... }
//     async fn handle(...) -> CommandResult<...> { ... }
// }

// ✨ AFTER: Simplified with enhanced derive macro (new way)
#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: u64,
}

#[async_trait]
impl CommandLogic for TransferMoney {
    type State = AccountBalances; // Manual (domain-specific)
    type Event = BankingEvent; // Manual (domain-specific)

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::MoneyTransferred { from, to, amount } => {
                let from_stream = StreamId::try_new(from).unwrap();
                let to_stream = StreamId::try_new(to).unwrap();
                state.debit(&from_stream, *amount);
                state.credit(&to_stream, *amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut eventcore::StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // For this demo, allow overdrafts (in real apps, you'd validate funds)
        println!(
            "Demo: Transferring {} from {} (balance: {}) to {}",
            self.amount,
            self.from_account.as_ref(),
            state.balance(&self.from_account),
            self.to_account.as_ref()
        );

        // Create the transfer event
        let event = StreamWrite::new(
            &read_streams,
            self.from_account.clone(),
            BankingEvent::MoneyTransferred {
                from: self.from_account.as_ref().to_string(),
                to: self.to_account.as_ref().to_string(),
                amount: self.amount,
            },
        )?;

        Ok(vec![event])
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use eventcore_memory::InMemoryEventStore;

    let store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(store);

    // Create accounts with initial funds
    let alice_account = StreamId::try_new("account-alice")?;
    let bob_account = StreamId::try_new("account-bob")?;

    // For this demo, let's modify the business logic to allow overdrafts
    // In a real application, you'd properly initialize accounts with funds

    // Transfer money from Alice to Bob
    let command = TransferMoney {
        from_account: alice_account,
        to_account: bob_account,
        amount: 100,
    };

    // Execute the command - no separate input needed
    let result = executor.execute(command, Default::default()).await?;
    println!("Transfer successful! Generated {} events", result.len());

    Ok(())
}
