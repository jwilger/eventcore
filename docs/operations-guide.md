# EventCore Operations Guide

This guide provides day-to-day operational procedures for managing EventCore-based applications in production. It covers monitoring, maintenance, troubleshooting, and incident response.

## Table of Contents

- [Daily Operations](#daily-operations)
- [Monitoring and Alerting](#monitoring-and-alerting)
- [Logging Best Practices](#logging-best-practices)
- [Performance Management](#performance-management)
- [Backup and Recovery](#backup-and-recovery)
- [Incident Response](#incident-response)
- [Maintenance Procedures](#maintenance-procedures)
- [Capacity Planning](#capacity-planning)
- [Security Operations](#security-operations)

## Daily Operations

### Health Check Routine

1. **System Health Dashboard Review**
   ```bash
   # Check overall system health
   curl -s http://app.example.com/health | jq .
   
   # Expected response
   {
     "status": "healthy",
     "version": "1.0.0",
     "checks": {
       "database": "healthy",
       "event_store": "healthy",
       "projections": "healthy"
     }
   }
   ```

2. **Key Metrics Review**
   - Command execution rate
   - Event store write throughput
   - Error rates
   - Response time percentiles (p50, p95, p99)
   - Database connection pool utilization

3. **Alert Review**
   - Acknowledge and investigate any overnight alerts
   - Update incident tracking system
   - Review alert fatigue and tune thresholds if needed

### Operational Checklists

#### Start of Day Checklist
- [ ] Review overnight alerts and incidents
- [ ] Check system health dashboards
- [ ] Verify backup completion
- [ ] Review error logs for anomalies
- [ ] Check projection lag metrics
- [ ] Verify scheduled job completion

#### End of Day Checklist
- [ ] Document any operational changes
- [ ] Update runbooks with new findings
- [ ] Hand off any open incidents
- [ ] Review tomorrow's maintenance schedule
- [ ] Ensure on-call rotation is correct

## Monitoring and Alerting

### Key Metrics to Monitor

#### Application Metrics

```yaml
# Prometheus alert rules
groups:
  - name: eventcore_alerts
    rules:
      - alert: HighCommandFailureRate
        expr: rate(eventcore_command_failures_total[5m]) > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High command failure rate"
          description: "Command failure rate is {{ $value }} (threshold: 0.05)"

      - alert: EventStoreLatencyHigh
        expr: histogram_quantile(0.95, eventcore_event_store_duration_seconds) > 0.5
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Event store latency is high"
          description: "95th percentile latency is {{ $value }}s"

      - alert: ProjectionLagHigh
        expr: eventcore_projection_lag_seconds > 300
        for: 15m
        labels:
          severity: critical
        annotations:
          summary: "Projection lag exceeds 5 minutes"
          description: "Projection {{ $labels.projection }} lag is {{ $value }}s"
```

#### System Metrics

```yaml
      - alert: DatabaseConnectionPoolExhausted
        expr: eventcore_db_connections_active / eventcore_db_connections_max > 0.9
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Database connection pool near exhaustion"
          description: "{{ $value | humanizePercentage }} of connections in use"

      - alert: MemoryUsageHigh
        expr: process_resident_memory_bytes / node_memory_MemTotal_bytes > 0.8
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Memory usage exceeds 80%"
          description: "Process using {{ $value | humanizePercentage }} of available memory"
```

### Dashboard Configuration

#### Grafana Dashboard Example

```json
{
  "dashboard": {
    "title": "EventCore Operations",
    "panels": [
      {
        "title": "Command Execution Rate",
        "targets": [{
          "expr": "rate(eventcore_commands_total[5m])"
        }]
      },
      {
        "title": "Command Success Rate",
        "targets": [{
          "expr": "1 - (rate(eventcore_command_failures_total[5m]) / rate(eventcore_commands_total[5m]))"
        }]
      },
      {
        "title": "Event Store Write Throughput",
        "targets": [{
          "expr": "rate(eventcore_events_written_total[5m])"
        }]
      },
      {
        "title": "Projection Lag",
        "targets": [{
          "expr": "eventcore_projection_lag_seconds"
        }]
      }
    ]
  }
}
```

## Logging Best Practices

### Structured Logging Configuration

```rust
// Configure structured logging for production
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true))
    .with(tracing_subscriber::EnvFilter::from_default_env())
    .init();
```

### Log Aggregation

```yaml
# Fluentd configuration for log collection
<source>
  @type tail
  path /var/log/eventcore/*.log
  pos_file /var/log/td-agent/eventcore.pos
  tag eventcore.*
  <parse>
    @type json
    time_key timestamp
    time_format %Y-%m-%dT%H:%M:%S.%NZ
  </parse>
</source>

<filter eventcore.**>
  @type record_transformer
  <record>
    hostname ${hostname}
    environment production
    service eventcore
  </record>
</filter>

<match eventcore.**>
  @type elasticsearch
  host elasticsearch.example.com
  port 9200
  index_name eventcore-%Y.%m.%d
  type_name _doc
</match>
```

### Log Analysis Queries

```sql
-- Find slow commands in logs
SELECT 
  timestamp,
  command_type,
  duration_ms,
  trace_id
FROM logs
WHERE 
  service = 'eventcore'
  AND level = 'INFO'
  AND message LIKE '%command completed%'
  AND duration_ms > 1000
ORDER BY duration_ms DESC
LIMIT 100;

-- Identify error patterns
SELECT 
  error_type,
  COUNT(*) as occurrence_count,
  MIN(timestamp) as first_seen,
  MAX(timestamp) as last_seen
FROM logs
WHERE 
  service = 'eventcore'
  AND level = 'ERROR'
  AND timestamp > NOW() - INTERVAL '24 hours'
GROUP BY error_type
ORDER BY occurrence_count DESC;
```

## Performance Management

### Performance Tuning Checklist

1. **Database Optimization**
   ```sql
   -- Check for missing indexes
   SELECT 
     schemaname,
     tablename,
     attname,
     n_distinct,
     most_common_vals
   FROM pg_stats
   WHERE tablename = 'events'
   AND n_distinct > 100;

   -- Identify slow queries
   SELECT 
     query,
     calls,
     mean_exec_time,
     total_exec_time
   FROM pg_stat_statements
   WHERE mean_exec_time > 100
   ORDER BY mean_exec_time DESC
   LIMIT 20;
   ```

2. **Connection Pool Tuning**
   ```bash
   # Monitor connection pool metrics
   curl -s http://localhost:9090/metrics | grep eventcore_db_connections
   
   # Adjust pool size based on utilization
   # If consistently > 80% utilized, increase pool size
   # If consistently < 30% utilized, decrease pool size
   ```

3. **Memory Profiling**
   ```bash
   # Generate heap profile
   RUST_LOG=debug eventcore-app &
   PID=$!
   sleep 60  # Let it run for a minute
   kill -SIGUSR1 $PID  # Trigger heap dump
   
   # Analyze with heaptrack or valgrind
   heaptrack --analyze heaptrack.eventcore-app.$PID.gz
   ```

### Performance Troubleshooting

#### High Latency Investigation

1. **Identify bottleneck layer**
   ```bash
   # Check command execution time breakdown
   SELECT 
     command_type,
     avg(total_duration_ms) as avg_total,
     avg(db_read_duration_ms) as avg_read,
     avg(processing_duration_ms) as avg_process,
     avg(db_write_duration_ms) as avg_write
   FROM command_metrics
   WHERE timestamp > NOW() - INTERVAL '1 hour'
   GROUP BY command_type
   ORDER BY avg_total DESC;
   ```

2. **Database query analysis**
   ```sql
   -- Enable query logging temporarily
   ALTER SYSTEM SET log_min_duration_statement = 100;
   SELECT pg_reload_conf();
   
   -- Analyze slow query log
   -- Look for sequential scans, missing indexes, lock contention
   ```

3. **Application profiling**
   ```bash
   # CPU profiling with perf
   perf record -F 99 -p $(pgrep eventcore-app) -g -- sleep 30
   perf report
   
   # Flamegraph generation
   perf script | stackcollapse-perf.pl | flamegraph.pl > flamegraph.svg
   ```

## Backup and Recovery

### Backup Procedures

#### Automated Backup Script

```bash
#!/bin/bash
# eventcore-backup.sh

set -euo pipefail

BACKUP_DIR="/backups/eventcore"
DB_NAME="eventcore"
RETENTION_DAYS=30

# Create backup directory
mkdir -p "${BACKUP_DIR}/$(date +%Y-%m-%d)"

# Backup database
echo "Starting database backup..."
pg_dump \
  --verbose \
  --format=custom \
  --compress=9 \
  --file="${BACKUP_DIR}/$(date +%Y-%m-%d)/eventcore-$(date +%H%M%S).dump" \
  "${DB_NAME}"

# Backup event store metadata
echo "Backing up event store metadata..."
psql -d "${DB_NAME}" -c "\COPY (SELECT * FROM event_store_metadata) TO '${BACKUP_DIR}/$(date +%Y-%m-%d)/metadata.csv' WITH CSV HEADER"

# Clean old backups
echo "Cleaning old backups..."
find "${BACKUP_DIR}" -type d -mtime +${RETENTION_DAYS} -exec rm -rf {} \;

echo "Backup completed successfully"
```

#### Recovery Procedures

```bash
#!/bin/bash
# eventcore-restore.sh

set -euo pipefail

BACKUP_FILE=$1
DB_NAME="eventcore_restore"

# Validate backup file
if [[ ! -f "${BACKUP_FILE}" ]]; then
    echo "Backup file not found: ${BACKUP_FILE}"
    exit 1
fi

# Create restore database
echo "Creating restore database..."
createdb "${DB_NAME}"

# Restore backup
echo "Restoring backup..."
pg_restore \
  --verbose \
  --dbname="${DB_NAME}" \
  --no-owner \
  --no-privileges \
  "${BACKUP_FILE}"

# Verify restore
echo "Verifying restore..."
psql -d "${DB_NAME}" -c "SELECT COUNT(*) FROM events;"

echo "Restore completed. Database: ${DB_NAME}"
```

### Disaster Recovery Plan

1. **RPO/RTO Targets**
   - Recovery Point Objective (RPO): 1 hour
   - Recovery Time Objective (RTO): 4 hours

2. **Recovery Procedures**
   - Database restoration from backup
   - Projection rebuild from events
   - Configuration restoration
   - DNS/load balancer updates

3. **Testing Schedule**
   - Monthly backup restoration test
   - Quarterly full DR drill
   - Annual region failover test

## Incident Response

### Incident Classification

| Severity | Description | Response Time | Examples |
|----------|-------------|---------------|----------|
| Critical | Complete service outage | < 15 minutes | Database down, all commands failing |
| High | Partial service degradation | < 30 minutes | High error rate, projection lag > 30min |
| Medium | Performance degradation | < 2 hours | Increased latency, projection lag > 5min |
| Low | Minor issues | < 24 hours | Non-critical errors, alerting issues |

### Incident Response Playbooks

#### Database Connection Exhaustion

```bash
# 1. Immediate mitigation
# Kill idle connections
psql -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE state = 'idle' AND state_change < NOW() - INTERVAL '5 minutes';"

# 2. Scale application if needed
kubectl scale deployment eventcore-app --replicas=10

# 3. Investigate root cause
# Check for connection leaks
psql -c "SELECT client_addr, state, COUNT(*) FROM pg_stat_activity GROUP BY client_addr, state ORDER BY COUNT(*) DESC;"

# 4. Long-term fix
# Adjust connection pool settings
# Add connection timeout
# Implement circuit breaker
```

#### High Command Failure Rate

```bash
# 1. Identify failing commands
curl -s http://localhost:9090/metrics | grep eventcore_command_failures_total | sort -k2 -nr

# 2. Check recent deployments
kubectl rollout history deployment/eventcore-app

# 3. Rollback if necessary
kubectl rollout undo deployment/eventcore-app

# 4. Investigate errors
kubectl logs -l app=eventcore-app --since=1h | grep ERROR | head -100

# 5. Apply fix and monitor
# Deploy fix
# Monitor error rate recovery
```

## Maintenance Procedures

### Routine Maintenance

#### Weekly Tasks

```bash
# 1. Database maintenance
psql -d eventcore -c "VACUUM ANALYZE events;"
psql -d eventcore -c "REINDEX INDEX CONCURRENTLY idx_events_stream_id_version;"

# 2. Log rotation verification
logrotate -d /etc/logrotate.d/eventcore

# 3. Certificate expiration check
openssl x509 -in /etc/ssl/certs/eventcore.crt -noout -dates

# 4. Disk space check
df -h | grep -E "(eventcore|postgres)"
```

#### Monthly Tasks

```bash
# 1. Security updates
cargo audit
cargo update --dry-run

# 2. Performance baseline update
# Run performance tests
# Compare with previous baseline
# Document any degradation

# 3. Backup restoration test
# Restore latest backup to test environment
# Verify data integrity
# Test application functionality
```

### Projection Maintenance

#### Rebuilding Projections

```rust
// Rebuild a specific projection
use eventcore::projection::{ProjectionManager, RebuildOptions};

let rebuild_options = RebuildOptions::default()
    .with_batch_size(1000)
    .with_checkpoint_interval(Duration::from_secs(60))
    .with_concurrency(4);

projection_manager
    .rebuild_projection("user_summary", rebuild_options)
    .await?;
```

#### Monitoring Rebuild Progress

```sql
-- Check projection rebuild status
SELECT 
  projection_name,
  status,
  events_processed,
  total_events,
  ROUND(events_processed::numeric / total_events * 100, 2) as progress_percent,
  started_at,
  CASE 
    WHEN events_processed > 0 
    THEN (EXTRACT(EPOCH FROM NOW() - started_at) / events_processed * (total_events - events_processed))::interval
    ELSE NULL
  END as estimated_time_remaining
FROM projection_rebuild_status
WHERE status = 'RUNNING';
```

## Capacity Planning

### Metrics for Capacity Planning

```sql
-- Event growth rate
SELECT 
  DATE_TRUNC('day', created_at) as day,
  COUNT(*) as events_count,
  pg_size_pretty(SUM(pg_column_size(payload))) as payload_size
FROM events
WHERE created_at > NOW() - INTERVAL '30 days'
GROUP BY day
ORDER BY day;

-- Stream growth rate
SELECT 
  DATE_TRUNC('week', MIN(created_at)) as week,
  COUNT(DISTINCT stream_id) as new_streams
FROM events
GROUP BY week
ORDER BY week;
```

### Scaling Triggers

| Metric | Threshold | Action |
|--------|-----------|--------|
| CPU Usage | > 70% sustained | Add CPU or scale horizontally |
| Memory Usage | > 80% sustained | Add memory or optimize |
| Storage Usage | > 70% | Expand storage or archive |
| Connection Pool | > 80% utilized | Increase pool size |
| Command Latency p99 | > 1s | Investigate and optimize |

### Capacity Forecasting

```python
# Simple linear forecasting script
import pandas as pd
from datetime import datetime, timedelta

# Load historical metrics
df = pd.read_csv('eventcore_metrics.csv', parse_dates=['date'])

# Calculate growth rate
daily_growth = df.groupby('date')['event_count'].sum().pct_change().mean()

# Forecast 90 days
current_rate = df['event_count'].iloc[-1]
forecast_days = 90
projected_rate = current_rate * ((1 + daily_growth) ** forecast_days)

print(f"Current event rate: {current_rate:,.0f}/day")
print(f"Projected rate in {forecast_days} days: {projected_rate:,.0f}/day")
print(f"Growth factor: {projected_rate/current_rate:.2f}x")
```

## Security Operations

### Security Monitoring

```yaml
# Security-focused alerts
- alert: UnauthorizedCommandAttempt
  expr: rate(eventcore_unauthorized_commands_total[5m]) > 0
  labels:
    severity: security
  annotations:
    summary: "Unauthorized command attempts detected"

- alert: AnomalousTrafficPattern
  expr: rate(eventcore_commands_total[5m]) > 10 * avg_over_time(rate(eventcore_commands_total[5m])[1h:5m])
  labels:
    severity: security
  annotations:
    summary: "Traffic spike detected - possible DDoS"
```

### Security Checklist

#### Daily Security Tasks
- [ ] Review authentication failures
- [ ] Check for unusual access patterns
- [ ] Verify certificate validity
- [ ] Monitor for CVE announcements

#### Weekly Security Tasks
- [ ] Run vulnerability scans
- [ ] Review access logs
- [ ] Update security dashboards
- [ ] Test incident response procedures

### Compliance and Auditing

```sql
-- Audit trail queries
-- Find all commands by user
SELECT 
  c.command_id,
  c.command_type,
  c.user_id,
  c.timestamp,
  c.result
FROM command_audit_log c
WHERE c.user_id = $1
  AND c.timestamp BETWEEN $2 AND $3
ORDER BY c.timestamp DESC;

-- Data retention compliance
-- Identify events older than retention period
SELECT 
  stream_id,
  COUNT(*) as event_count,
  MIN(created_at) as oldest_event,
  MAX(created_at) as newest_event
FROM events
WHERE created_at < NOW() - INTERVAL '7 years'
GROUP BY stream_id
ORDER BY oldest_event;
```

## Runbook References

### Quick Reference Commands

```bash
# Health checks
curl -s http://localhost:8080/health | jq .
curl -s http://localhost:9090/metrics | grep eventcore_

# Database queries
psql -d eventcore -c "SELECT pg_size_pretty(pg_database_size('eventcore'));"
psql -d eventcore -c "SELECT COUNT(*) FROM events;"

# Kubernetes operations
kubectl get pods -l app=eventcore-app
kubectl logs -f deployment/eventcore-app
kubectl exec -it deployment/eventcore-app -- /bin/bash

# Performance investigation
top -p $(pgrep eventcore-app)
netstat -an | grep ESTABLISHED | grep 5432 | wc -l
iostat -x 1
```

### Emergency Contacts

- On-call engineer: Check PagerDuty
- Database team: db-team@example.com
- Infrastructure team: infra-team@example.com
- Security team: security@example.com

## Additional Resources

- [Deployment Guide](deployment-guide.md) - Deployment strategies and configurations
- [Troubleshooting Guide](troubleshooting.md) - Common issues and solutions
- [Performance Monitoring](performance-monitoring.md) - Detailed performance analysis
- [Monitoring and Observability](monitoring-and-observability.md) - Metrics and tracing setup