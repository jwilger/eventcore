# Compliance

EventCore's immutable audit trail helps with compliance, but you must implement specific controls.

> **📋 Comprehensive Compliance Checklist**
>
> For a detailed compliance checklist covering OWASP, NIST, SOC2, PCI DSS, GDPR, and HIPAA requirements, see our [COMPLIANCE_CHECKLIST.md](https://github.com/jwilger/eventcore/blob/main/COMPLIANCE_CHECKLIST.md).
>
> This checklist provides actionable items for achieving compliance with major security frameworks and regulations.

## GDPR Compliance

### Data Protection Principles

1. **Lawfulness**: Store only data with legal basis
2. **Purpose Limitation**: Use data only for stated purposes
3. **Data Minimization**: Store only necessary data
4. **Accuracy**: Provide mechanisms to correct data
5. **Storage Limitation**: Implement retention policies
6. **Security**: Encrypt and protect personal data

### Right to Erasure (Right to be Forgotten)

Since events are immutable, use crypto-shredding:

```rust
use std::collections::HashMap;
use uuid::Uuid;

struct GdprCompliantEventStore {
    event_store: Box<dyn EventStore>,
    key_vault: Box<dyn KeyVault>,
    user_keys: HashMap<UserId, KeyId>,
}

impl GdprCompliantEventStore {
    async fn forget_user(&mut self, user_id: UserId) -> Result<(), Error> {
        // 1. Delete user's encryption key
        if let Some(key_id) = self.user_keys.remove(&user_id) {
            self.key_vault.delete_key(key_id).await?;
        }

        // 2. Store erasure event for audit
        let erasure_event = UserDataErased {
            user_id: user_id.clone(),
            erased_at: Timestamp::now(),
            reason: "GDPR Article 17 Request".to_string(),
        };

        self.event_store
            .append_events(
                &StreamId::from_user(&user_id),
                vec![Event::from(erasure_event)],
            )
            .await?;

        // 3. Events remain but PII is now unreadable
        Ok(())
    }
}
```

### Data Portability

Export user data in machine-readable format:

```rust
#[async_trait]
trait GdprExport {
    async fn export_user_data(
        &self,
        user_id: UserId,
    ) -> Result<UserDataExport, Error>;
}

#[derive(Serialize)]
struct UserDataExport {
    user_id: UserId,
    export_date: Timestamp,
    profile: UserProfile,
    events: Vec<UserEvent>,
    projections: HashMap<String, Value>,
}

impl EventStore {
    async fn export_user_events(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<UserEvent>, Error> {
        // Collect all events related to user
        let streams = self.find_user_streams(user_id).await?;
        let mut events = Vec::new();

        for stream_id in streams {
            let stream_events = self.read_stream(&stream_id).await?;
            events.extend(
                stream_events
                    .into_iter()
                    .filter(|e| e.involves_user(user_id))
                    .map(|e| e.decrypt_for_export())
            );
        }

        Ok(events)
    }
}
```

## PCI DSS Compliance

### Never Store in Events

```rust
// BAD - Never do this
#[derive(Serialize, Deserialize)]
struct PaymentProcessed {
    card_number: String,      // NEVER!
    cvv: String,             // NEVER!
    pin: String,             // NEVER!
}

// GOOD - Store only tokens
#[derive(Serialize, Deserialize)]
struct PaymentProcessed {
    payment_id: PaymentId,
    card_token: CardToken,    // From PCI-compliant tokenizer
    last_four: String,        // "****1234"
    amount: Money,
    merchant_ref: String,
}
```

### Audit Requirements

```rust
struct PciAuditLogger {
    logger: Box<dyn AuditLogger>,
}

impl PciAuditLogger {
    async fn log_payment_access(
        &self,
        user: &User,
        action: PaymentAction,
        resource: &str,
    ) -> Result<(), Error> {
        let entry = AuditEntry {
            timestamp: Timestamp::now(),
            user_id: user.id.clone(),
            action: action.to_string(),
            resource: resource.to_string(),
            ip_address: user.ip_address.clone(),
            success: true,
        };

        self.logger.log(entry).await
    }
}
```

## HIPAA Compliance

### Protected Health Information (PHI)

Always encrypt PHI:

```rust
#[derive(Serialize, Deserialize)]
struct PatientRecord {
    patient_id: PatientId,
    // All PHI must be encrypted
    encrypted_name: EncryptedField,
    encrypted_ssn: EncryptedField,
    encrypted_diagnosis: EncryptedField,
    encrypted_medications: EncryptedField,
    // Non-PHI can be unencrypted
    admission_date: Date,
    room_number: String,
}

struct HipaaCompliantStore {
    encryption: EncryptionService,
    audit: AuditService,
}

impl HipaaCompliantStore {
    async fn store_patient_event(
        &self,
        event: PatientEvent,
        accessed_by: UserId,
    ) -> Result<(), Error> {
        // Audit the access
        self.audit.log_phi_access(
            &accessed_by,
            &event.patient_id(),
            "WRITE",
        ).await?;

        // Encrypt and store
        let encrypted = self.encryption.encrypt_event(event)?;
        self.event_store.append(encrypted).await?;

        Ok(())
    }
}
```

### Access Controls

```rust
#[derive(Debug, Clone)]
enum HipaaRole {
    Doctor,
    Nurse,
    Admin,
    Billing,
}

impl HipaaRole {
    fn can_access_phi(&self) -> bool {
        matches!(self, HipaaRole::Doctor | HipaaRole::Nurse)
    }

    fn can_access_billing(&self) -> bool {
        matches!(self, HipaaRole::Admin | HipaaRole::Billing)
    }
}
```

## SOX Compliance

### Financial Controls

```rust
struct SoxCompliantExecutor {
    executor: CommandExecutor,
    approvals: ApprovalService,
}

impl SoxCompliantExecutor {
    async fn execute_financial_command(
        &self,
        command: FinancialCommand,
        requester: User,
    ) -> Result<(), Error> {
        // Segregation of duties
        if command.amount() > Money::from_dollars(10_000) {
            let approver = self.approvals
                .get_approver(&requester)
                .await?;

            self.approvals
                .request_approval(&command, &approver)
                .await?;
        }

        // Execute with full audit trail
        let result = self.executor
            .execute_with_metadata(
                command,
                metadata! {
                    "sox_requester" => requester.id,
                    "sox_timestamp" => Timestamp::now(),
                    "sox_ip" => requester.ip_address,
                },
            )
            .await?;

        Ok(result)
    }
}
```

## General Compliance Features

### Audit Trail

```rust
#[derive(Debug, Serialize)]
struct ComplianceAuditEntry {
    timestamp: Timestamp,
    event_id: EventId,
    stream_id: StreamId,
    user_id: UserId,
    action: String,
    regulation: String, // "GDPR", "PCI", "HIPAA"
    details: HashMap<String, String>,
}

trait ComplianceAuditor {
    async fn log_access(&self, entry: ComplianceAuditEntry) -> Result<(), Error>;
    async fn generate_report(
        &self,
        regulation: &str,
        from: Date,
        to: Date,
    ) -> Result<ComplianceReport, Error>;
}
```

### Data Retention

```rust
struct RetentionPolicy {
    regulation: String,
    data_type: String,
    retention_days: u32,
    action: RetentionAction,
}

enum RetentionAction {
    Delete,
    Archive,
    Anonymize,
}

struct RetentionManager {
    policies: Vec<RetentionPolicy>,
}

impl RetentionManager {
    async fn apply_retention(&self, event_store: &EventStore) -> Result<(), Error> {
        for policy in &self.policies {
            let cutoff = Timestamp::now() - Duration::days(policy.retention_days);

            match policy.action {
                RetentionAction::Delete => {
                    // For GDPR compliance
                    self.crypto_shred_old_data(cutoff).await?;
                }
                RetentionAction::Archive => {
                    // Move to cold storage
                    self.archive_old_events(cutoff).await?;
                }
                RetentionAction::Anonymize => {
                    // Remove PII but keep analytics data
                    self.anonymize_old_events(cutoff).await?;
                }
            }
        }
        Ok(())
    }
}
```

## Compliance Checklist

- [ ] Implement encryption for all PII/PHI
- [ ] Set up audit logging for all access
- [ ] Configure data retention policies
- [ ] Implement right to erasure (GDPR)
- [ ] Set up data export capabilities
- [ ] Configure access controls (RBAC/ABAC)
- [ ] Implement approval workflows (SOX)
- [ ] Set up monitoring and alerting
- [ ] Document all compliance measures
- [ ] Regular compliance audits
