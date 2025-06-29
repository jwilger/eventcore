//! Projections for the banking domain
//!
//! Projections build read models from events for querying.

use crate::banking::{
    events::BankingEvent,
    types::{AccountId, Money},
};
use async_trait::async_trait;
use eventcore::{
    Event, Projection, ProjectionCheckpoint, ProjectionConfig, ProjectionError, ProjectionResult,
    ProjectionStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Account balance projection tracking current balance for all accounts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountBalanceProjection {
    /// Current balance for each account
    pub balances: HashMap<AccountId, AccountBalance>,
}

/// Balance information for a single account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    /// The account ID
    pub account_id: AccountId,
    /// Current balance
    pub balance: Money,
    /// Number of transactions
    pub transaction_count: u64,
    /// Total deposited
    pub total_deposited: Money,
    /// Total withdrawn
    pub total_withdrawn: Money,
}

impl AccountBalance {
    /// Creates a new account balance
    pub fn new(account_id: AccountId, initial_balance: Money) -> Self {
        Self {
            account_id,
            balance: initial_balance,
            transaction_count: 0,
            total_deposited: initial_balance,
            total_withdrawn: Money::zero(),
        }
    }
}

/// Implementation of the account balance projection
#[derive(Debug)]
pub struct AccountBalanceProjectionImpl {
    config: ProjectionConfig,
    state: AccountBalanceProjection,
    checkpoint: ProjectionCheckpoint,
    status: ProjectionStatus,
}

impl Default for AccountBalanceProjectionImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountBalanceProjectionImpl {
    /// Creates a new account balance projection
    pub fn new() -> Self {
        Self {
            config: ProjectionConfig::new("account_balances"),
            state: AccountBalanceProjection::default(),
            checkpoint: ProjectionCheckpoint::initial(),
            status: ProjectionStatus::Stopped,
        }
    }

    /// Applies an event to the projection state
    #[allow(clippy::unused_async)]
    pub async fn apply_event(&mut self, event: &Event<BankingEvent>) -> ProjectionResult<()> {
        match &event.payload {
            BankingEvent::AccountOpened(e) => {
                let balance = AccountBalance::new(e.account_id.clone(), e.initial_balance);
                self.state.balances.insert(e.account_id.clone(), balance);
            }
            BankingEvent::MoneyTransferred(e) => {
                // Update sender's balance
                if let Some(from_balance) = self.state.balances.get_mut(&e.from_account) {
                    from_balance.balance =
                        from_balance.balance.subtract(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                    from_balance.transaction_count += 1;
                    from_balance.total_withdrawn =
                        from_balance.total_withdrawn.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }

                // Update receiver's balance
                if let Some(to_balance) = self.state.balances.get_mut(&e.to_account) {
                    to_balance.balance = to_balance.balance.add(&e.amount).map_err(|e| {
                        ProjectionError::Internal(format!("Balance calculation error: {e}"))
                    })?;
                    to_balance.transaction_count += 1;
                    to_balance.total_deposited =
                        to_balance.total_deposited.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }
            }
            BankingEvent::MoneyDeposited(e) => {
                if let Some(balance) = self.state.balances.get_mut(&e.account_id) {
                    balance.balance = balance.balance.add(&e.amount).map_err(|e| {
                        ProjectionError::Internal(format!("Balance calculation error: {e}"))
                    })?;
                    balance.transaction_count += 1;
                    balance.total_deposited =
                        balance.total_deposited.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }
            }
            BankingEvent::MoneyWithdrawn(e) => {
                if let Some(balance) = self.state.balances.get_mut(&e.account_id) {
                    balance.balance = balance.balance.subtract(&e.amount).map_err(|e| {
                        ProjectionError::Internal(format!("Balance calculation error: {e}"))
                    })?;
                    balance.transaction_count += 1;
                    balance.total_withdrawn =
                        balance.total_withdrawn.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Projection for AccountBalanceProjectionImpl {
    type Event = BankingEvent;
    type State = AccountBalanceProjection;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        Ok(self.state.clone())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        Ok(self.status)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        Ok(self.checkpoint.clone())
    }

    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        // In a real implementation, this would persist to storage
        // For the example, we just ignore the checkpoint
        let _ = checkpoint;
        Ok(())
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        match &event.payload {
            BankingEvent::AccountOpened(e) => {
                let balance = AccountBalance::new(e.account_id.clone(), e.initial_balance);
                state.balances.insert(e.account_id.clone(), balance);
            }
            BankingEvent::MoneyTransferred(e) => {
                // Update sender's balance
                if let Some(from_balance) = state.balances.get_mut(&e.from_account) {
                    from_balance.balance =
                        from_balance.balance.subtract(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                    from_balance.transaction_count += 1;
                    from_balance.total_withdrawn =
                        from_balance.total_withdrawn.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }

                // Update receiver's balance
                if let Some(to_balance) = state.balances.get_mut(&e.to_account) {
                    to_balance.balance = to_balance.balance.add(&e.amount).map_err(|e| {
                        ProjectionError::Internal(format!("Balance calculation error: {e}"))
                    })?;
                    to_balance.transaction_count += 1;
                    to_balance.total_deposited =
                        to_balance.total_deposited.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }
            }
            BankingEvent::MoneyDeposited(e) => {
                if let Some(balance) = state.balances.get_mut(&e.account_id) {
                    balance.balance = balance.balance.add(&e.amount).map_err(|e| {
                        ProjectionError::Internal(format!("Balance calculation error: {e}"))
                    })?;
                    balance.transaction_count += 1;
                    balance.total_deposited =
                        balance.total_deposited.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }
            }
            BankingEvent::MoneyWithdrawn(e) => {
                if let Some(balance) = state.balances.get_mut(&e.account_id) {
                    balance.balance = balance.balance.subtract(&e.amount).map_err(|e| {
                        ProjectionError::Internal(format!("Balance calculation error: {e}"))
                    })?;
                    balance.transaction_count += 1;
                    balance.total_withdrawn =
                        balance.total_withdrawn.add(&e.amount).map_err(|e| {
                            ProjectionError::Internal(format!("Balance calculation error: {e}"))
                        })?;
                }
            }
        }
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(AccountBalanceProjection::default())
    }
}

/// Summary statistics for the entire banking system
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BankingSystemStats {
    /// Total number of accounts
    pub total_accounts: u64,
    /// Total money in the system
    pub total_balance: Money,
    /// Total number of transfers
    pub total_transfers: u64,
    /// Total amount transferred
    pub total_transferred: Money,
}

/// Projection for system-wide statistics
#[derive(Debug, Clone)]
pub struct SystemStatsProjection {
    config: ProjectionConfig,
    stats: BankingSystemStats,
    checkpoint: ProjectionCheckpoint,
    status: ProjectionStatus,
}

impl Default for SystemStatsProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemStatsProjection {
    /// Creates a new system stats projection
    pub fn new() -> Self {
        Self {
            config: ProjectionConfig::new("banking_system_stats"),
            stats: BankingSystemStats::default(),
            checkpoint: ProjectionCheckpoint::initial(),
            status: ProjectionStatus::Stopped,
        }
    }
}

#[async_trait]
impl Projection for SystemStatsProjection {
    type Event = BankingEvent;
    type State = BankingSystemStats;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        Ok(self.stats.clone())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        Ok(self.status)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        Ok(self.checkpoint.clone())
    }

    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        // In a real implementation, this would persist to storage
        let _ = checkpoint;
        Ok(())
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        match &event.payload {
            BankingEvent::AccountOpened(e) => {
                state.total_accounts += 1;
                state.total_balance = state.total_balance.add(&e.initial_balance).map_err(|e| {
                    ProjectionError::Internal(format!("Balance calculation error: {e}"))
                })?;
            }
            BankingEvent::MoneyTransferred(e) => {
                state.total_transfers += 1;
                state.total_transferred = state.total_transferred.add(&e.amount).map_err(|e| {
                    ProjectionError::Internal(format!("Balance calculation error: {e}"))
                })?;
            }
            BankingEvent::MoneyDeposited(e) => {
                state.total_balance = state.total_balance.add(&e.amount).map_err(|e| {
                    ProjectionError::Internal(format!("Balance calculation error: {e}"))
                })?;
            }
            BankingEvent::MoneyWithdrawn(e) => {
                state.total_balance = state.total_balance.subtract(&e.amount).map_err(|e| {
                    ProjectionError::Internal(format!("Balance calculation error: {e}"))
                })?;
            }
        }
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(BankingSystemStats::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::banking::{
        events::AccountOpened,
        types::{AccountHolder, CustomerName},
    };
    use eventcore::{EventId, EventMetadata, StreamId, Timestamp};

    fn create_test_event<E>(payload: E) -> Event<E>
    where
        E: PartialEq + Eq,
    {
        Event {
            id: EventId::new(),
            stream_id: StreamId::try_new("test-stream".to_string()).unwrap(),
            payload,
            metadata: EventMetadata::new(),
            created_at: Timestamp::now(),
        }
    }

    #[tokio::test]
    async fn account_balance_projection_handles_account_opened() {
        let projection = AccountBalanceProjectionImpl::new();

        let event = create_test_event(BankingEvent::AccountOpened(AccountOpened {
            account_id: AccountId::try_new("ACC-123".to_string()).unwrap(),
            holder: AccountHolder {
                name: CustomerName::try_new("Test User".to_string()).unwrap(),
                email: "test@example.com".to_string(),
            },
            initial_balance: Money::from_cents(10000).unwrap(),
        }));

        let mut state = AccountBalanceProjection::default();
        projection.apply_event(&mut state, &event).await.unwrap();

        let balance = state
            .balances
            .get(&AccountId::try_new("ACC-123".to_string()).unwrap())
            .unwrap();

        assert_eq!(balance.balance, Money::from_cents(10000).unwrap());
        assert_eq!(balance.total_deposited, Money::from_cents(10000).unwrap());
        assert_eq!(balance.total_withdrawn, Money::zero());
    }
}
