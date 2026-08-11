# Chapter 6.3: Backup and Recovery

Data protection is critical for EventCore applications since event stores contain the complete history of your system. This chapter covers comprehensive backup strategies, disaster recovery procedures, and data integrity verification.

> **What EventCore provides vs. what your infrastructure owns.** EventCore
> exposes **no backup, restore, export, or import API**. There is no
> `BackupManager`, no `PointInTimeRecovery`, no `EventStoreBackup`, and the
> `EventStore`/`EventReader` traits have no `list_all_streams`,
> `read_events_since`, `clear_all`, or `write_events` methods. The only way to
> append events is through a command's `handle()` persisted by
> `eventcore::execute()`; the only way to read them is `read_stream` (per
> stream, for state reconstruction) and `read_events` (the global read used by
> projections).
>
> Because events are **immutable and append-only**, the right place to back up
> an EventCore system is the **storage layer**, using the backup tooling that
> ships with your backend:
>
> - **PostgreSQL** — `pg_dump`/`pg_restore`, continuous WAL archiving, and
>   point-in-time recovery (PITR) via Barman, CloudNativePG, or your managed
>   database's snapshot facility.
> - **SQLite** — file copy / filesystem snapshot of the database file (use the
>   SQLite Online Backup API or `VACUUM INTO` for a consistent copy while the
>   DB is open).
> - **File store (`eventcore-fs`)** — copy or snapshot the store directory.
>
> Everything in this chapter that looks like Rust orchestration
> (`DisasterRecoveryOrchestrator`, health checks) is **application/operations
> code you write around EventCore**, not an API the library exports. The
> EventCore-specific health calls it uses (`ping()`, `migrate()`) are real and
> documented below.

## Backup Strategies

Append-only event stores are unusually friendly to backup: events are never
mutated or deleted in normal operation, so a storage-layer snapshot plus
continuous WAL archiving captures the entire system history with no
application coordination required.

### PostgreSQL Backup Configuration

The PostgreSQL backend stores events in ordinary tables, so any
PostgreSQL-native backup approach works. The example below uses CloudNativePG
with continuous WAL archiving to object storage, which gives you both periodic
base backups and point-in-time recovery:

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
  schedule: "0 2 * * *" # Daily at 2 AM
  backupOwnerReference: self
  cluster:
    name: eventcore-postgres
  target: prefer-standby
  method: barmanObjectStore
```

### Manual `pg_dump` Backups

For environments without an operator, a scheduled `pg_dump` of the EventCore
database is sufficient for full backups. Because events are append-only, a
consistent dump captures a complete, replayable history:

```bash
# Full logical backup of the EventCore database
pg_dump \
  --format=custom \
  --compress=9 \
  --file="eventcore-$(date +%Y%m%dT%H%M%S).dump" \
  "$DATABASE_URL"

# Restore into a fresh database
createdb eventcore_restored
pg_restore \
  --dbname="postgresql://.../eventcore_restored" \
  --no-owner \
  eventcore-20260101T020000.dump
```

For point-in-time recovery between base backups, enable WAL archiving
(`archive_mode = on`, `archive_command = '...'`) and use standard PostgreSQL
PITR (`recovery_target_time`). This is the same PITR mechanism CloudNativePG
automates above — there is no EventCore-specific recovery step.

### SQLite Backup Configuration

The SQLite backend (`eventcore-sqlite`) stores events in a single database
file. Back it up the way you back up any SQLite database:

```bash
# Consistent copy while the database may be in use:
sqlite3 eventcore.db "VACUUM INTO 'eventcore-backup.db'"

# Or, when the application is stopped, a plain file copy / filesystem snapshot
cp eventcore.db eventcore-backup-$(date +%Y%m%d).db
```

At-rest encryption for SQLite is a **storage concern**, not something the
application does per event. `eventcore-sqlite` offers an optional, non-default
`encryption` feature backed by SQLCipher
(`encryption = ["rusqlite/bundled-sqlcipher-vendored-openssl"]`); enable it and
supply the key at open time if you need an encrypted database file. Back up the
encrypted file the same way — the ciphertext is what you copy.

### File Store Backup Configuration

The file backend (`eventcore-fs`), opened with `FileEventStore::open(path)`,
stores events under a directory tree. Back it up with a directory copy or a
filesystem-level snapshot (LVM, ZFS, btrfs, or a cloud volume snapshot). As
with the other backends, immutability means a snapshot taken at any moment is a
consistent, replayable history.

## Disaster Recovery

### Multi-Region Backup Strategy

Geographic distribution of backups is an infrastructure concern, configured
where your backups live (object storage replication, cross-region snapshot
copy). The following illustrates a policy expressed as a ConfigMap; it does not
involve any EventCore API:

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

### Disaster Recovery Orchestration (application/operations code)

The recovery _decisions_ — assess the failure, pick a strategy, fail over to a
replica region, update DNS — are operational logic you own. EventCore's only
role is providing the health signals you check against a store. The orchestrator
below is **your code**; the only EventCore calls it makes are the real backend
health methods (`PostgresEventStore::ping()` and `migrate()`):

```rust
// Application/operations code — NOT an EventCore API.
#[derive(Clone)]
pub struct DisasterRecoveryOrchestrator {
    primary_region: String,
    failover_regions: Vec<String>,
    // Your infrastructure-control clients (DNS, autoscaling, alerting) go here.
}

impl DisasterRecoveryOrchestrator {
    pub async fn execute_disaster_recovery(
        &self,
        trigger: DisasterTrigger,
    ) -> Result<RecoveryOutcome, DisasterRecoveryError> {
        tracing::error!(trigger = ?trigger, "Disaster recovery triggered");

        // Assess the situation
        let assessment = self.assess_disaster_scope().await?;

        // Choose recovery strategy
        let strategy = self.choose_recovery_strategy(&assessment).await?;

        // Execute recovery
        match strategy {
            RecoveryStrategy::LocalRestore => self.execute_local_restore().await,
            RecoveryStrategy::RegionalFailover { target_region } => {
                self.execute_regional_failover(&target_region).await
            }
            RecoveryStrategy::FullRebuild => self.execute_full_rebuild().await,
        }
    }

    async fn assess_disaster_scope(&self) -> Result<DisasterAssessment, DisasterRecoveryError> {
        let mut assessment = DisasterAssessment::default();

        // Check primary database health. PostgresEventStore::ping() runs a
        // trivial query and PANICS if the database is unreachable — it returns
        // (), not a Result. Catch the panic (or wrap construction) to turn an
        // unreachable database into a boolean signal.
        assessment.primary_db_accessible =
            self.check_store_health(&self.primary_region).await;

        // Check replica regions the same way.
        for region in &self.failover_regions {
            let accessible = self.check_store_health(region).await;
            assessment.replica_regions.insert(region.clone(), accessible);
        }

        // Estimate data loss from your backup/replication lag metrics.
        assessment.estimated_data_loss = self.calculate_potential_data_loss().await?;

        Ok(assessment)
    }

    /// Probe a region's PostgreSQL store. `ping()` returns () and panics on
    /// failure, so we treat a successful return as healthy and a caught panic
    /// (or a connection error during construction) as unhealthy.
    async fn check_store_health(&self, region: &str) -> bool {
        let connection_string = self.store_url_for_region(region);
        match eventcore_postgres::PostgresEventStore::new(connection_string).await {
            Ok(store) => {
                // ping() panics on failure; isolate it so a dead region does
                // not take down the orchestrator.
                let probe = tokio::task::spawn(async move { store.ping().await });
                probe.await.is_ok()
            }
            Err(_) => false,
        }
    }

    async fn execute_regional_failover(
        &self,
        target_region: &str,
    ) -> Result<RecoveryOutcome, DisasterRecoveryError> {
        tracing::info!(target_region, "Executing regional failover");

        // 1. Promote the replica's database in the target region (storage-layer
        //    operation: CloudNativePG promotion, managed-DB failover, etc.).
        self.promote_replica(target_region).await?;

        // 2. Run EventCore's schema migration against the promoted database so
        //    the schema is current before serving traffic. migrate() returns ()
        //    and panics on failure.
        let store = eventcore_postgres::PostgresEventStore::new(
            self.store_url_for_region(target_region),
        )
        .await
        .map_err(DisasterRecoveryError::StoreUnavailable)?;
        store.migrate().await;

        // 3. Update DNS to point at the new region.
        self.update_dns_routing(target_region).await?;

        // 4. Scale up resources in the target region.
        self.scale_up_target_region(target_region).await?;

        // 5. Verify and notify.
        let health_check = self.verify_system_health(target_region).await?;
        self.notify_failover_completion(target_region, &health_check).await?;

        Ok(RecoveryOutcome {
            strategy_used: RecoveryStrategy::RegionalFailover {
                target_region: target_region.to_string(),
            },
            data_loss_minutes: 0, // Assuming near-real-time replication
            systems_recovered: health_check.systems_operational,
        })
    }
}

#[derive(Debug, Default)]
pub struct DisasterAssessment {
    pub primary_db_accessible: bool,
    pub replica_regions: std::collections::HashMap<String, bool>,
    pub estimated_data_loss: std::time::Duration,
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

The key point: failover _restores the storage layer_ (a promoted replica or a
restored `pg_dump`/snapshot), and EventCore picks up exactly where the events
leave off. There is no event-level restore loop to write, because there is no
event-level backup API to begin with.

## Data Integrity Verification

EventCore guarantees integrity at write time: `append_events` performs atomic,
multi-stream writes under optimistic concurrency control, so a stream's
versions are gap-free and monotonically increasing by construction
(`StreamVersion`), and projection progress is captured by `StreamPosition`.
For filesystem stores after Git merges, that cursor is replica-local ingestion
order rather than a shared global timestamp. Events are immutable once written.
There is no application-level
"event corruption monitor" API, and EventCore does not expose a way to mutate
or re-checksum stored events.

### Verify Backups at the Storage Layer

Verify the integrity of a backup the way you verify any database backup —
without an EventCore-specific tool:

- **PostgreSQL.** Restore the dump into a throwaway database
  (`pg_restore`), then run consistency checks. `pg_restore --list` and
  `pg_dump`'s own custom-format checksums catch a truncated or corrupt archive.
  WAL archives are validated by PITR replay.
- **SQLite.** Run `PRAGMA integrity_check;` (or `PRAGMA quick_check;`) against
  the backup file. A clean result means the file is structurally sound.
- **File store.** Verify your snapshot/copy with the filesystem's checksum or
  the object store's ETag/MD5 on upload.

### Verify Replayability with EventCore

The strongest end-to-end check is to confirm a restored store actually replays.
Restore into a scratch database, point a backend at it, and rebuild a
projection with the real entry point:

```rust
// Restore the storage layer first (pg_restore / file copy), then:
let store = eventcore_postgres::PostgresEventStore::new(restored_db_url).await?;
store.migrate().await; // returns (); panics on failure

// Rebuild a read model from the restored events via the canonical entry point.
// run_projection borrows the backend; execute()/run_projection() are the only
// ways events enter or leave the store.
eventcore::run_projection(my_projector, &store, ProjectionConfig::default()).await?;
```

If the projection rebuilds to the expected state, the restored events are
intact and replayable. This exercises the same `read_events` path the
application uses in production, which is a far stronger guarantee than checking
individual rows.

> **Recovering individual streams.** To inspect a single stream after a
> restore, read it back with `EventStore::read_stream::<MyEvent>(stream_id)`,
> where `stream_id` is built with `StreamId::try_new(...)`. This is a read for
> verification only — you cannot re-append those events directly. New events
> are produced exclusively by a command's `handle()` and persisted by
> `eventcore::execute()`.

## Backup Testing and Validation

Test recovery, not just backup creation. The most valuable rehearsal is a
full restore-and-replay drill, run on a schedule, that exercises the real
storage tooling and the real EventCore replay path:

1. **Restore drill.** Restore the latest base backup (and replay WAL to a
   target time, for PostgreSQL PITR) into an isolated environment using
   `pg_restore` / file copy. Confirm the restore completes without error.
2. **Schema check.** Run `store.migrate()` against the restored database and
   confirm it returns (does not panic), proving the schema matches the code.
3. **Replay check.** Run `eventcore::run_projection(...)` against the restored
   store and assert the rebuilt read model matches expectations. This is the
   acceptance test for "the backup is usable."
4. **Cross-region check.** Periodically run the same drill against a backup
   copied to a secondary region to validate cross-region recoverability.

Because these drills use only real, published EventCore entry points
(`run_projection`, `read_stream`, `migrate`, `ping`) plus storage-native
tooling (`pg_restore`, `PRAGMA integrity_check`, file copy), they validate the
exact path a real recovery takes — there is no bespoke backup framework to keep
in sync with the library.

## Best Practices

1. **Regular backups** - Automated, frequent storage-layer backup schedules
   (`pg_dump`/WAL archiving, SQLite file snapshots, file-store snapshots)
2. **Point-in-time recovery** - Enable WAL archiving for PostgreSQL PITR
3. **Geographic distribution** - Replicate backups to multiple regions
4. **Regular restore drills** - Test restore _and_ replay, not just backup
5. **Replayability checks** - Rebuild a projection with `run_projection` against
   a restored store to prove events are intact
6. **Recovery planning** - Documented disaster recovery procedures
7. **Retention policies** - Appropriate data retention and archival
8. **Encryption at rest** - A storage concern: SQLite via the `encryption`
   (SQLCipher) feature, PostgreSQL/file store via disk or tablespace encryption

## Summary

EventCore backup and recovery:

- ✅ **Storage-layer backups** - `pg_dump`/WAL archiving, SQLite/file snapshots
- ✅ **Immutable, append-only events** - snapshots are consistent by construction
- ✅ **Point-in-time recovery** - via PostgreSQL WAL replay (Barman/CloudNativePG)
- ✅ **Replay-based verification** - rebuild projections with `run_projection`
- ✅ **Disaster recovery** - operations code that restores storage and lets
  EventCore resume from the restored events

Key points:

1. EventCore has **no backup/restore API** — back up the storage layer
2. Use the backend's native tooling (`pg_dump`/PITR, SQLite snapshots, file copy)
3. Verify backups with storage-native checks, then prove replayability with
   `run_projection` against a restored store
4. Run restore-and-replay drills regularly, including cross-region copies
5. Treat at-rest encryption as a storage concern (SQLCipher / disk encryption)

Next, let's explore [Troubleshooting](./04-troubleshooting.md) →
