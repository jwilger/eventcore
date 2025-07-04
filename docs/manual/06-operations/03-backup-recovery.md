# Chapter 6.3: Backup and Recovery

Data protection is critical for EventCore applications since event stores contain the complete history of your system. This chapter covers comprehensive backup strategies, disaster recovery procedures, and data integrity verification.

## Backup Strategies

### PostgreSQL Backup Configuration

EventCore's PostgreSQL event store requires specific backup considerations:

```yaml
# PostgreSQL backup configuration using CloudNativePG
apiVersion: postgresql.cnpg.io/v1
kind: Cluster
metadata:
  name: eventcore-postgres
  namespace: eventcore
spec:
  instances: 3
  
  backup:
    target: prefer-standby
    retentionPolicy: "30d"
    
    # Base backup configuration
    data:
      compression: gzip
      encryption: AES256
      jobs: 2
      immediateCheckpoint: true
    
    # WAL archiving
    wal:
      compression: gzip
      encryption: AES256
      maxParallel: 2
    
    # Backup schedule
    barmanObjectStore:
      destinationPath: "s3://eventcore-backups/postgres"
      s3Credentials:
        accessKeyId:
          name: backup-credentials
          key: ACCESS_KEY_ID
        secretAccessKey:
          name: backup-credentials
          key: SECRET_ACCESS_KEY
      wal:
        retention: "7d"
      data:
        retention: "30d"
        jobs: 2
---
apiVersion: postgresql.cnpg.io/v1
kind: ScheduledBackup
metadata:
  name: eventcore-backup-schedule
  namespace: eventcore
spec:
  schedule: "0 2 * * *"  # Daily at 2 AM
  backupOwnerReference: self
  cluster:
    name: eventcore-postgres
  target: prefer-standby
  method: barmanObjectStore
```

### Event Store Backup Implementation

```rust
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BackupManager {
    event_store: Arc<dyn EventStore>,
    storage: Arc<dyn BackupStorage>,
    config: BackupConfig,
}

#[derive(Debug, Clone)]
pub struct BackupConfig {
    pub backup_format: BackupFormat,
    pub compression: CompressionType,
    pub encryption_enabled: bool,
    pub chunk_size: usize,
    pub retention_days: u32,
    pub verify_after_backup: bool,
}

#[derive(Debug, Clone)]
pub enum BackupFormat {
    JsonLines,
    MessagePack,
    Custom,
}

#[derive(Debug, Clone)]
pub enum CompressionType {
    None,
    Gzip,
    Zstd,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub backup_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub format: BackupFormat,
    pub compression: CompressionType,
    pub total_events: u64,
    pub total_streams: u64,
    pub size_bytes: u64,
    pub checksum: String,
    pub event_range: EventRange,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventRange {
    pub earliest_event: DateTime<Utc>,
    pub latest_event: DateTime<Utc>,
    pub earliest_version: EventVersion,
    pub latest_version: EventVersion,
}

impl BackupManager {
    pub async fn create_full_backup(&self) -> Result<BackupMetadata, BackupError> {
        let backup_id = Uuid::new_v4();
        let start_time = Utc::now();
        
        tracing::info!(backup_id = %backup_id, "Starting full backup");
        
        // Create backup metadata
        let mut metadata = BackupMetadata {
            backup_id,
            created_at: start_time,
            format: self.config.backup_format.clone(),
            compression: self.config.compression.clone(),
            total_events: 0,
            total_streams: 0,
            size_bytes: 0,
            checksum: String::new(),
            event_range: EventRange {
                earliest_event: start_time,
                latest_event: start_time,
                earliest_version: EventVersion::initial(),
                latest_version: EventVersion::initial(),
            },
        };
        
        // Get all streams
        let streams = self.event_store.list_all_streams().await?;
        metadata.total_streams = streams.len() as u64;
        
        // Create backup writer
        let backup_path = format!("full-backup-{}.eventcore", backup_id);
        let mut writer = BackupWriter::new(
            &backup_path,
            self.config.compression.clone(),
            self.config.encryption_enabled,
        ).await?;
        
        // Write backup header
        writer.write_header(&metadata).await?;
        
        // Backup each stream
        for stream_id in streams {
            let events = self.backup_stream(&stream_id, &mut writer).await?;
            metadata.total_events += events;
            
            if metadata.total_events % 10000 == 0 {
                tracing::info!(
                    backup_id = %backup_id,
                    events_backed_up = metadata.total_events,
                    "Backup progress"
                );
            }
        }
        
        // Calculate checksums and finalize
        metadata.size_bytes = writer.finalize().await?;
        metadata.checksum = writer.calculate_checksum().await?;
        
        // Store backup metadata
        self.storage.store_backup(&backup_path, &metadata).await?;
        
        // Verify backup if configured
        if self.config.verify_after_backup {
            self.verify_backup(&backup_id).await?;
        }
        
        let duration = Utc::now().signed_duration_since(start_time);
        tracing::info!(
            backup_id = %backup_id,
            duration_seconds = duration.num_seconds(),
            total_events = metadata.total_events,
            size_mb = metadata.size_bytes / (1024 * 1024),
            "Backup completed successfully"
        );
        
        Ok(metadata)
    }
    
    pub async fn create_incremental_backup(
        &self,
        since: DateTime<Utc>,
    ) -> Result<BackupMetadata, BackupError> {
        let backup_id = Uuid::new_v4();
        let start_time = Utc::now();
        
        tracing::info!(
            backup_id = %backup_id,
            since = %since,
            "Starting incremental backup"
        );
        
        // Query events since timestamp
        let events = self.event_store.read_events_since(since).await?;
        
        let mut metadata = BackupMetadata {
            backup_id,
            created_at: start_time,
            format: self.config.backup_format.clone(),
            compression: self.config.compression.clone(),
            total_events: events.len() as u64,
            total_streams: 0, // Will be calculated
            size_bytes: 0,
            checksum: String::new(),
            event_range: self.calculate_event_range(&events),
        };
        
        // Create backup writer
        let backup_path = format!("incremental-backup-{}.eventcore", backup_id);
        let mut writer = BackupWriter::new(
            &backup_path,
            self.config.compression.clone(),
            self.config.encryption_enabled,
        ).await?;
        
        // Write incremental backup
        writer.write_header(&metadata).await?;
        
        let mut unique_streams = std::collections::HashSet::new();
        for event in events {
            writer.write_event(&event).await?;
            unique_streams.insert(event.stream_id.clone());
        }
        
        metadata.total_streams = unique_streams.len() as u64;
        metadata.size_bytes = writer.finalize().await?;
        metadata.checksum = writer.calculate_checksum().await?;
        
        self.storage.store_backup(&backup_path, &metadata).await?;
        
        tracing::info!(
            backup_id = %backup_id,
            total_events = metadata.total_events,
            total_streams = metadata.total_streams,
            "Incremental backup completed"
        );
        
        Ok(metadata)
    }
    
    async fn backup_stream(
        &self,
        stream_id: &StreamId,
        writer: &mut BackupWriter,
    ) -> Result<u64, BackupError> {
        let mut event_count = 0;
        let mut from_version = EventVersion::initial();
        let batch_size = self.config.chunk_size;
        
        loop {
            let options = ReadOptions::default()
                .from_version(from_version)
                .limit(batch_size);
            
            let stream_events = self.event_store.read_stream(stream_id, options).await?;
            
            if stream_events.events.is_empty() {
                break;
            }
            
            for event in &stream_events.events {
                writer.write_event(event).await?;
                event_count += 1;
            }
            
            from_version = EventVersion::from(
                stream_events.events.last().unwrap().version.as_u64() + 1
            );
        }
        
        Ok(event_count)
    }
    
    fn calculate_event_range(&self, events: &[StoredEvent]) -> EventRange {
        if events.is_empty() {
            let now = Utc::now();
            return EventRange {
                earliest_event: now,
                latest_event: now,
                earliest_version: EventVersion::initial(),
                latest_version: EventVersion::initial(),
            };
        }
        
        let earliest = events.iter().min_by_key(|e| e.occurred_at).unwrap();
        let latest = events.iter().max_by_key(|e| e.occurred_at).unwrap();
        
        EventRange {
            earliest_event: earliest.occurred_at,
            latest_event: latest.occurred_at,
            earliest_version: earliest.version,
            latest_version: latest.version,
        }
    }
}

struct BackupWriter {
    file: BufWriter<File>,
    path: String,
    compression: CompressionType,
    encrypted: bool,
    bytes_written: u64,
}

impl BackupWriter {
    async fn new(
        path: &str,
        compression: CompressionType,
        encrypted: bool,
    ) -> Result<Self, BackupError> {
        let file = File::create(path).await?;
        let file = BufWriter::new(file);
        
        Ok(Self {
            file,
            path: path.to_string(),
            compression,
            encrypted,
            bytes_written: 0,
        })
    }
    
    async fn write_header(&mut self, metadata: &BackupMetadata) -> Result<(), BackupError> {
        let header = serde_json::to_string(metadata)?;
        let header_line = format!("EVENTCORE_BACKUP_HEADER:{}\n", header);
        
        self.file.write_all(header_line.as_bytes()).await?;
        self.bytes_written += header_line.len() as u64;
        
        Ok(())
    }
    
    async fn write_event(&mut self, event: &StoredEvent) -> Result<(), BackupError> {
        let event_line = match self.compression {
            CompressionType::None => {
                let json = serde_json::to_string(event)?;
                format!("{}\n", json)
            }
            CompressionType::Gzip => {
                // Implement gzip compression
                let json = serde_json::to_string(event)?;
                format!("{}\n", json) // Simplified for example
            }
            CompressionType::Zstd => {
                // Implement zstd compression
                let json = serde_json::to_string(event)?;
                format!("{}\n", json) // Simplified for example
            }
        };
        
        self.file.write_all(event_line.as_bytes()).await?;
        self.bytes_written += event_line.len() as u64;
        
        Ok(())
    }
    
    async fn finalize(&mut self) -> Result<u64, BackupError> {
        self.file.flush().await?;
        Ok(self.bytes_written)
    }
    
    async fn calculate_checksum(&self) -> Result<String, BackupError> {
        // Calculate SHA-256 checksum of the backup file
        use sha2::{Sha256, Digest};
        use tokio::fs::File;
        use tokio::io::AsyncReadExt;
        
        let mut file = File::open(&self.path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];
        
        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }
}
```

### Point-in-Time Recovery

```rust
#[derive(Debug, Clone)]
pub struct PointInTimeRecovery {
    backup_manager: BackupManager,
    event_store: Arc<dyn EventStore>,
}

impl PointInTimeRecovery {
    pub async fn restore_to_point_in_time(
        &self,
        target_time: DateTime<Utc>,
    ) -> Result<RecoveryResult, RecoveryError> {
        tracing::info!(target_time = %target_time, "Starting point-in-time recovery");
        
        // Find the best backup to start from
        let base_backup = self.find_best_base_backup(target_time).await?;
        
        // Restore from base backup
        self.restore_from_backup(&base_backup.backup_id).await?;
        
        // Apply incremental backups up to the target time
        let incremental_backups = self.find_incremental_backups_until(
            base_backup.created_at,
            target_time,
        ).await?;
        
        for backup in incremental_backups {
            self.apply_incremental_backup(&backup.backup_id, Some(target_time)).await?;
        }
        
        // Apply WAL entries up to the exact target time
        self.apply_wal_entries_until(target_time).await?;
        
        // Verify recovery
        let recovery_result = self.verify_recovery(target_time).await?;
        
        tracing::info!(
            target_time = %target_time,
            events_restored = recovery_result.events_restored,
            streams_restored = recovery_result.streams_restored,
            "Point-in-time recovery completed"
        );
        
        Ok(recovery_result)
    }
    
    async fn find_best_base_backup(
        &self,
        target_time: DateTime<Utc>,
    ) -> Result<BackupMetadata, RecoveryError> {
        let backups = self.backup_manager.list_backups().await?;
        
        // Find the latest full backup before the target time
        let base_backup = backups
            .iter()
            .filter(|b| b.created_at <= target_time)
            .filter(|b| matches!(b.format, BackupFormat::JsonLines)) // Full backup indicator
            .max_by_key(|b| b.created_at)
            .ok_or(RecoveryError::NoSuitableBackup)?;
        
        Ok(base_backup.clone())
    }
    
    async fn restore_from_backup(&self, backup_id: &Uuid) -> Result<(), RecoveryError> {
        tracing::info!(backup_id = %backup_id, "Restoring from base backup");
        
        // Clear the event store
        self.event_store.clear_all().await?;
        
        // Read backup file
        let backup_reader = BackupReader::new(backup_id).await?;
        let metadata = backup_reader.read_metadata().await?;
        
        tracing::info!(
            backup_id = %backup_id,
            total_events = metadata.total_events,
            "Reading backup events"
        );
        
        // Restore events in batches
        let batch_size = 1000;
        let mut events_restored = 0;
        
        while let Some(batch) = backup_reader.read_events_batch(batch_size).await? {
            self.event_store.write_events(batch).await?;
            events_restored += batch_size;
            
            if events_restored % 10000 == 0 {
                tracing::info!(
                    events_restored = events_restored,
                    "Restore progress"
                );
            }
        }
        
        Ok(())
    }
    
    async fn apply_wal_entries_until(
        &self,
        target_time: DateTime<Utc>,
    ) -> Result<(), RecoveryError> {
        // Apply WAL (Write-Ahead Log) entries from PostgreSQL
        // This provides exact point-in-time recovery
        
        let wal_entries = self.read_wal_entries_until(target_time).await?;
        
        for entry in wal_entries {
            if entry.timestamp <= target_time {
                self.apply_wal_entry(entry).await?;
            }
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryResult {
    pub events_restored: u64,
    pub streams_restored: u64,
    pub recovery_time: DateTime<Utc>,
    pub data_integrity_verified: bool,
}
```

## Disaster Recovery

### Multi-Region Backup Strategy

```yaml
# Multi-region backup configuration
apiVersion: v1
kind: ConfigMap
metadata:
  name: backup-config
  namespace: eventcore
data:
  backup-policy.yaml: |
    # Primary backup configuration
    primary:
      region: us-east-1
      storage: s3://eventcore-backups-primary
      schedule: "0 */6 * * *"  # Every 6 hours
      retention: "30d"
      
    # Cross-region replication
    replicas:
      - region: us-west-2
        storage: s3://eventcore-backups-west
        sync_schedule: "0 1 * * *"  # Daily sync
        retention: "90d"
        
      - region: eu-west-1
        storage: s3://eventcore-backups-eu
        sync_schedule: "0 2 * * *"  # Daily sync
        retention: "90d"
    
    # Archive configuration
    archive:
      storage: glacier://eventcore-archive
      after_days: 90
      retention: "7y"
```

### Automated Disaster Recovery

```rust
#[derive(Debug, Clone)]
pub struct DisasterRecoveryOrchestrator {
    primary_region: String,
    failover_regions: Vec<String>,
    backup_manager: BackupManager,
    health_checker: HealthChecker,
}

impl DisasterRecoveryOrchestrator {
    pub async fn execute_disaster_recovery(
        &self,
        trigger: DisasterTrigger,
    ) -> Result<RecoveryOutcome, DisasterRecoveryError> {
        tracing::error!(
            trigger = ?trigger,
            "Disaster recovery triggered"
        );
        
        // Assess the situation
        let assessment = self.assess_disaster_scope().await?;
        
        // Choose recovery strategy
        let strategy = self.choose_recovery_strategy(&assessment).await?;
        
        // Execute recovery
        match strategy {
            RecoveryStrategy::LocalRestore => {
                self.execute_local_restore().await
            }
            RecoveryStrategy::RegionalFailover { target_region } => {
                self.execute_regional_failover(&target_region).await
            }
            RecoveryStrategy::FullRebuild => {
                self.execute_full_rebuild().await
            }
        }
    }
    
    async fn assess_disaster_scope(&self) -> Result<DisasterAssessment, DisasterRecoveryError> {
        let mut assessment = DisasterAssessment::default();
        
        // Check primary database
        assessment.primary_db_accessible = self.health_checker
            .check_database_connectivity(&self.primary_region)
            .await
            .is_ok();
        
        // Check backup availability
        assessment.backup_accessible = self.backup_manager
            .verify_backup_accessibility()
            .await
            .is_ok();
        
        // Check replica regions
        for region in &self.failover_regions {
            let accessible = self.health_checker
                .check_database_connectivity(region)
                .await
                .is_ok();
            assessment.replica_regions.insert(region.clone(), accessible);
        }
        
        // Estimate data loss
        assessment.estimated_data_loss = self.calculate_potential_data_loss().await?;
        
        Ok(assessment)
    }
    
    async fn execute_regional_failover(
        &self,
        target_region: &str,
    ) -> Result<RecoveryOutcome, DisasterRecoveryError> {
        tracing::info!(
            target_region = target_region,
            "Executing regional failover"
        );
        
        // 1. Promote replica in target region
        self.promote_replica(target_region).await?;
        
        // 2. Update DNS to point to new region
        self.update_dns_routing(target_region).await?;
        
        // 3. Scale up resources in target region
        self.scale_up_target_region(target_region).await?;
        
        // 4. Verify system health
        let health_check = self.verify_system_health(target_region).await?;
        
        // 5. Notify stakeholders
        self.notify_failover_completion(target_region, &health_check).await?;
        
        Ok(RecoveryOutcome {
            strategy_used: RecoveryStrategy::RegionalFailover {
                target_region: target_region.to_string(),
            },
            recovery_time: Utc::now(),
            data_loss_minutes: 0, // Assuming near-real-time replication
            systems_recovered: health_check.systems_operational,
        })
    }
}

#[derive(Debug)]
pub struct DisasterAssessment {
    pub primary_db_accessible: bool,
    pub backup_accessible: bool,
    pub replica_regions: HashMap<String, bool>,
    pub estimated_data_loss: Duration,
}

#[derive(Debug, Clone)]
pub enum RecoveryStrategy {
    LocalRestore,
    RegionalFailover { target_region: String },
    FullRebuild,
}

#[derive(Debug)]
pub enum DisasterTrigger {
    DatabaseFailure,
    RegionOutage,
    DataCorruption,
    SecurityBreach,
    ManualTrigger,
}
```

## Data Integrity Verification

### Backup Verification

```rust
#[derive(Debug, Clone)]
pub struct BackupVerifier {
    event_store: Arc<dyn EventStore>,
    backup_storage: Arc<dyn BackupStorage>,
}

impl BackupVerifier {
    pub async fn verify_backup_integrity(
        &self,
        backup_id: &Uuid,
    ) -> Result<VerificationResult, VerificationError> {
        tracing::info!(backup_id = %backup_id, "Starting backup verification");
        
        let mut result = VerificationResult::default();
        
        // Verify checksum
        result.checksum_valid = self.verify_checksum(backup_id).await?;
        
        // Verify metadata consistency
        result.metadata_consistent = self.verify_metadata(backup_id).await?;
        
        // Verify event integrity
        result.events_valid = self.verify_events(backup_id).await?;
        
        // Verify completeness (if verifying against live system)
        if let Ok(completeness) = self.verify_completeness(backup_id).await {
            result.completeness_verified = true;
            result.missing_events = completeness.missing_events;
        }
        
        result.verification_time = Utc::now();
        result.overall_valid = result.checksum_valid &&
            result.metadata_consistent &&
            result.events_valid &&
            result.missing_events == 0;
        
        if result.overall_valid {
            tracing::info!(backup_id = %backup_id, "Backup verification passed");
        } else {
            tracing::error!(
                backup_id = %backup_id,
                result = ?result,
                "Backup verification failed"
            );
        }
        
        Ok(result)
    }
    
    async fn verify_checksum(&self, backup_id: &Uuid) -> Result<bool, VerificationError> {
        let backup_metadata = self.backup_storage.get_metadata(backup_id).await?;
        let calculated_checksum = self.calculate_backup_checksum(backup_id).await?;
        
        Ok(backup_metadata.checksum == calculated_checksum)
    }
    
    async fn verify_events(&self, backup_id: &Uuid) -> Result<bool, VerificationError> {
        let backup_reader = BackupReader::new(backup_id).await?;
        let mut events_valid = true;
        let mut event_count = 0;
        
        while let Some(event) = backup_reader.read_next_event().await? {
            // Verify event structure
            if !self.is_event_structurally_valid(&event) {
                tracing::error!(
                    backup_id = %backup_id,
                    event_id = %event.id,
                    "Invalid event structure found"
                );
                events_valid = false;
                break;
            }
            
            // Verify event ordering (within stream)
            if !self.is_event_ordering_valid(&event) {
                tracing::error!(
                    backup_id = %backup_id,
                    event_id = %event.id,
                    "Invalid event ordering found"
                );
                events_valid = false;
                break;
            }
            
            event_count += 1;
            
            if event_count % 10000 == 0 {
                tracing::info!(
                    backup_id = %backup_id,
                    events_verified = event_count,
                    "Verification progress"
                );
            }
        }
        
        Ok(events_valid)
    }
    
    fn is_event_structurally_valid(&self, event: &StoredEvent) -> bool {
        // Verify required fields
        if event.id.is_nil() || event.stream_id.as_ref().is_empty() {
            return false;
        }
        
        // Verify event ordering within stream
        if event.version.as_u64() == 0 {
            return false;
        }
        
        // Verify timestamp is reasonable
        let now = Utc::now();
        if event.occurred_at > now || event.occurred_at < (now - chrono::Duration::days(3650)) {
            return false;
        }
        
        true
    }
    
    fn is_event_ordering_valid(&self, event: &StoredEvent) -> bool {
        // This would need to track ordering within streams
        // Simplified implementation for example
        true
    }
}

#[derive(Debug, Default)]
pub struct VerificationResult {
    pub checksum_valid: bool,
    pub metadata_consistent: bool,
    pub events_valid: bool,
    pub completeness_verified: bool,
    pub missing_events: u64,
    pub verification_time: DateTime<Utc>,
    pub overall_valid: bool,
}
```

### Continuous Integrity Monitoring

```rust
#[derive(Debug, Clone)]
pub struct IntegrityMonitor {
    event_store: Arc<dyn EventStore>,
    monitoring_config: IntegrityMonitoringConfig,
}

#[derive(Debug, Clone)]
pub struct IntegrityMonitoringConfig {
    pub check_interval: Duration,
    pub sample_percentage: f64,
    pub alert_on_corruption: bool,
    pub auto_repair: bool,
}

impl IntegrityMonitor {
    pub async fn start_monitoring(&self) -> Result<(), MonitoringError> {
        tracing::info!("Starting continuous integrity monitoring");
        
        let mut interval = tokio::time::interval(self.monitoring_config.check_interval);
        
        loop {
            interval.tick().await;
            
            match self.perform_integrity_check().await {
                Ok(report) => {
                    if !report.integrity_ok {
                        tracing::error!(
                            corruption_count = report.corrupted_events,
                            "Data integrity issues detected"
                        );
                        
                        if self.monitoring_config.alert_on_corruption {
                            self.send_corruption_alert(&report).await;
                        }
                        
                        if self.monitoring_config.auto_repair {
                            self.attempt_auto_repair(&report).await;
                        }
                    } else {
                        tracing::debug!("Integrity check passed");
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Integrity check failed");
                }
            }
        }
    }
    
    async fn perform_integrity_check(&self) -> Result<IntegrityReport, MonitoringError> {
        let start_time = Utc::now();
        let mut report = IntegrityReport::default();
        
        // Sample events for checking
        let sample_events = self.sample_events().await?;
        report.events_checked = sample_events.len() as u64;
        
        for event in sample_events {
            // Check event integrity
            let integrity_check = self.check_event_integrity(&event).await?;
            
            if !integrity_check.valid {
                report.corrupted_events += 1;
                report.corruption_details.push(integrity_check);
            }
        }
        
        report.check_time = Utc::now();
        report.check_duration = report.check_time.signed_duration_since(start_time);
        report.integrity_ok = report.corrupted_events == 0;
        
        Ok(report)
    }
    
    async fn sample_events(&self) -> Result<Vec<StoredEvent>, MonitoringError> {
        // Sample a percentage of events for integrity checking
        let sample_size = ((self.get_total_event_count().await? as f64) 
            * self.monitoring_config.sample_percentage / 100.0) as usize;
        
        // Use reservoir sampling or similar technique
        self.event_store.sample_events(sample_size).await
            .map_err(MonitoringError::EventStoreError)
    }
    
    async fn check_event_integrity(&self, event: &StoredEvent) -> Result<EventIntegrityCheck, MonitoringError> {
        let mut check = EventIntegrityCheck {
            event_id: event.id,
            stream_id: event.stream_id.clone(),
            valid: true,
            issues: Vec::new(),
        };
        
        // Check payload can be deserialized
        if let Err(_) = serde_json::from_value::<serde_json::Value>(event.payload.clone()) {
            check.valid = false;
            check.issues.push("Payload deserialization failed".to_string());
        }
        
        // Check metadata is valid
        if event.metadata.is_empty() {
            check.issues.push("Missing metadata".to_string());
        }
        
        // Check event ordering within stream
        if let Err(_) = self.verify_event_ordering(event).await {
            check.valid = false;
            check.issues.push("Event ordering violation".to_string());
        }
        
        Ok(check)
    }
}

#[derive(Debug, Default)]
pub struct IntegrityReport {
    pub check_time: DateTime<Utc>,
    pub check_duration: chrono::Duration,
    pub events_checked: u64,
    pub corrupted_events: u64,
    pub integrity_ok: bool,
    pub corruption_details: Vec<EventIntegrityCheck>,
}

#[derive(Debug)]
pub struct EventIntegrityCheck {
    pub event_id: EventId,
    pub stream_id: StreamId,
    pub valid: bool,
    pub issues: Vec<String>,
}
```

## Backup Testing and Validation

### Automated Backup Testing

```rust
#[derive(Debug, Clone)]
pub struct BackupTestSuite {
    backup_manager: BackupManager,
    test_event_store: Arc<dyn EventStore>,
    test_config: BackupTestConfig,
}

#[derive(Debug, Clone)]
pub struct BackupTestConfig {
    pub test_frequency: Duration,
    pub full_restore_test_frequency: Duration,
    pub sample_restore_percentage: f64,
    pub cleanup_test_data: bool,
}

impl BackupTestSuite {
    pub async fn run_comprehensive_backup_tests(&self) -> Result<TestResults, TestError> {
        tracing::info!("Starting comprehensive backup tests");
        
        let mut results = TestResults::default();
        
        // Test 1: Backup creation
        results.backup_creation = self.test_backup_creation().await?;
        
        // Test 2: Backup verification
        results.backup_verification = self.test_backup_verification().await?;
        
        // Test 3: Partial restore
        results.partial_restore = self.test_partial_restore().await?;
        
        // Test 4: Full restore (if scheduled)
        if self.should_run_full_restore_test().await? {
            results.full_restore = Some(self.test_full_restore().await?);
        }
        
        // Test 5: Point-in-time recovery
        results.point_in_time_recovery = self.test_point_in_time_recovery().await?;
        
        // Test 6: Cross-region restore
        results.cross_region_restore = self.test_cross_region_restore().await?;
        
        results.overall_success = results.all_tests_passed();
        results.test_time = Utc::now();
        
        if results.overall_success {
            tracing::info!("All backup tests passed");
        } else {
            tracing::error!(results = ?results, "Some backup tests failed");
        }
        
        Ok(results)
    }
    
    async fn test_backup_creation(&self) -> Result<TestResult, TestError> {
        let start_time = Utc::now();
        
        // Create test data
        let test_events = self.create_test_events(1000).await?;
        self.write_test_events(&test_events).await?;
        
        // Create backup
        let backup_result = self.backup_manager.create_full_backup().await;
        
        let duration = Utc::now().signed_duration_since(start_time);
        
        match backup_result {
            Ok(metadata) => {
                Ok(TestResult {
                    test_name: "backup_creation".to_string(),
                    success: true,
                    duration,
                    details: format!("Backup created: {}", metadata.backup_id),
                    error: None,
                })
            }
            Err(e) => {
                Ok(TestResult {
                    test_name: "backup_creation".to_string(),
                    success: false,
                    duration,
                    details: "Backup creation failed".to_string(),
                    error: Some(e.to_string()),
                })
            }
        }
    }
    
    async fn test_full_restore(&self) -> Result<TestResult, TestError> {
        let start_time = Utc::now();
        
        // Get latest backup
        let latest_backup = self.backup_manager.get_latest_backup().await?;
        
        // Create clean test environment
        let test_store = self.create_clean_test_store().await?;
        
        // Perform restore
        let restore_result = self.restore_backup_to_store(
            &latest_backup.backup_id,
            &test_store,
        ).await;
        
        let duration = Utc::now().signed_duration_since(start_time);
        
        match restore_result {
            Ok(_) => {
                // Verify restore completeness
                let verification = self.verify_restore_completeness(&test_store).await?;
                
                Ok(TestResult {
                    test_name: "full_restore".to_string(),
                    success: verification.complete,
                    duration,
                    details: format!(
                        "Events restored: {}, Streams restored: {}",
                        verification.events_count,
                        verification.streams_count
                    ),
                    error: None,
                })
            }
            Err(e) => {
                Ok(TestResult {
                    test_name: "full_restore".to_string(),
                    success: false,
                    duration,
                    details: "Full restore failed".to_string(),
                    error: Some(e.to_string()),
                })
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct TestResults {
    pub backup_creation: TestResult,
    pub backup_verification: TestResult,
    pub partial_restore: TestResult,
    pub full_restore: Option<TestResult>,
    pub point_in_time_recovery: TestResult,
    pub cross_region_restore: TestResult,
    pub overall_success: bool,
    pub test_time: DateTime<Utc>,
}

impl TestResults {
    fn all_tests_passed(&self) -> bool {
        self.backup_creation.success &&
        self.backup_verification.success &&
        self.partial_restore.success &&
        self.full_restore.as_ref().map_or(true, |t| t.success) &&
        self.point_in_time_recovery.success &&
        self.cross_region_restore.success
    }
}

#[derive(Debug, Default)]
pub struct TestResult {
    pub test_name: String,
    pub success: bool,
    pub duration: chrono::Duration,
    pub details: String,
    pub error: Option<String>,
}
```

## Best Practices

1. **Regular backups** - Automated, frequent backup schedules
2. **Multiple strategies** - Full, incremental, and WAL-based backups
3. **Geographic distribution** - Multi-region backup storage
4. **Regular testing** - Automated backup and restore testing
5. **Integrity verification** - Continuous data integrity monitoring
6. **Recovery planning** - Documented disaster recovery procedures
7. **Retention policies** - Appropriate data retention and archival
8. **Security** - Encrypted backups and secure storage

## Summary

EventCore backup and recovery:

- ✅ **Comprehensive backups** - Full, incremental, and point-in-time
- ✅ **Disaster recovery** - Multi-region failover capabilities
- ✅ **Data integrity** - Continuous verification and monitoring
- ✅ **Automated testing** - Regular backup and restore validation
- ✅ **Recovery orchestration** - Automated disaster recovery procedures

Key components:
1. Implement automated backup strategies with multiple approaches
2. Design disaster recovery procedures for various failure scenarios
3. Continuously monitor data integrity with automated verification
4. Test backup and recovery procedures regularly
5. Maintain geographic distribution of backups for resilience

Next, let's explore [Troubleshooting](./04-troubleshooting.md) →