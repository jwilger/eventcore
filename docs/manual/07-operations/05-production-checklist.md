# Chapter 6.5: Production Checklist

This chapter provides a comprehensive checklist for deploying EventCore applications to production. Use this as a final validation before going live and as a periodic review for existing production systems.

## Pre-Deployment Checklist

### Security

#### Authentication and Authorization

- [ ] **JWT secret key** configured and secured
- [ ] **Token expiration** properly configured
- [ ] **Role-based access control** implemented and tested
- [ ] **API rate limiting** configured
- [ ] **CORS origins** restricted to known domains
- [ ] **HTTPS** enforced for all endpoints
- [ ] **Security headers** configured (HSTS, CSP, etc.)

```rust
// Security configuration validation
#[derive(Debug)]
pub struct SecurityAudit {
    pub findings: Vec<SecurityFinding>,
}

#[derive(Debug)]
pub struct SecurityFinding {
    pub category: SecurityCategory,
    pub severity: SecuritySeverity,
    pub description: String,
    pub recommendation: String,
}

#[derive(Debug)]
pub enum SecurityCategory {
    Authentication,
    Authorization,
    Encryption,
    NetworkSecurity,
    DataProtection,
}

#[derive(Debug)]
pub enum SecuritySeverity {
    Critical,
    High,
    Medium,
    Low,
}

pub struct SecurityAuditor;

impl SecurityAuditor {
    pub fn audit_configuration(config: &AppConfig) -> SecurityAudit {
        let mut findings = Vec::new();

        // Check JWT configuration
        if config.jwt.secret_key.len() < 32 {
            findings.push(SecurityFinding {
                category: SecurityCategory::Authentication,
                severity: SecuritySeverity::Critical,
                description: "JWT secret key is too short".to_string(),
                recommendation: "Use a secret key of at least 256 bits (32 bytes)".to_string(),
            });
        }

        // Check CORS configuration
        if config.cors.allowed_origins.contains(&"*".to_string()) {
            findings.push(SecurityFinding {
                category: SecurityCategory::NetworkSecurity,
                severity: SecuritySeverity::High,
                description: "CORS allows all origins".to_string(),
                recommendation: "Restrict CORS to specific trusted domains".to_string(),
            });
        }

        // Check HTTPS enforcement
        if !config.server.force_https {
            findings.push(SecurityFinding {
                category: SecurityCategory::NetworkSecurity,
                severity: SecuritySeverity::High,
                description: "HTTPS not enforced".to_string(),
                recommendation: "Enable HTTPS enforcement for all endpoints".to_string(),
            });
        }

        // Check rate limiting
        if config.rate_limiting.requests_per_minute == 0 {
            findings.push(SecurityFinding {
                category: SecurityCategory::NetworkSecurity,
                severity: SecuritySeverity::Medium,
                description: "Rate limiting not configured".to_string(),
                recommendation: "Configure appropriate rate limits for API endpoints".to_string(),
            });
        }

        SecurityAudit { findings }
    }
}
```

#### Database Security

- [ ] **Database credentials** stored in secrets management
- [ ] **Connection encryption** (SSL/TLS) enabled
- [ ] **Database user permissions** follow principle of least privilege
- [ ] **Database firewall rules** restrict access
- [ ] **Connection pooling** properly configured
- [ ] **Query parameterization** used (prevent SQL injection)

```sql
-- PostgreSQL security checklist queries
-- Check SSL is enforced
SHOW ssl;

-- Check user permissions
\du

-- Check database-level permissions
SELECT datname, datacl FROM pg_database;

-- Check table-level permissions
SELECT schemaname, tablename, tableowner, tablespace, hasindexes, hasrules, hastriggers
FROM pg_tables
WHERE schemaname = 'public';

-- Verify no wildcard permissions
SELECT * FROM information_schema.table_privileges
WHERE grantee = 'PUBLIC';
```

### Performance

#### Resource Limits

- [ ] **CPU limits** set appropriately
- [ ] **Memory limits** configured with buffer
- [ ] **Database connection pool** sized correctly
- [ ] **Request timeouts** configured
- [ ] **Circuit breakers** implemented
- [ ] **Resource quotas** set at namespace level

```yaml
# Kubernetes resource configuration checklist
apiVersion: v1
kind: LimitRange
metadata:
  name: eventcore-limits
  namespace: eventcore
spec:
  limits:
    - type: Container
      default:
        memory: "512Mi"
        cpu: "500m"
      defaultRequest:
        memory: "256Mi"
        cpu: "250m"
      max:
        memory: "2Gi"
        cpu: "2000m"
---
apiVersion: v1
kind: ResourceQuota
metadata:
  name: eventcore-quota
  namespace: eventcore
spec:
  hard:
    requests.cpu: "4"
    requests.memory: 8Gi
    limits.cpu: "8"
    limits.memory: 16Gi
    persistentvolumeclaims: "4"
```

#### Performance Benchmarks

- [ ] **Load testing** completed with realistic scenarios
- [ ] **Performance baselines** established
- [ ] **Scalability limits** identified
- [ ] **Database query performance** optimized
- [ ] **Index usage** analyzed and optimized

```rust
// Performance validation
pub struct PerformanceValidator {
    target_metrics: PerformanceTargets,
}

#[derive(Debug, Clone)]
pub struct PerformanceTargets {
    pub max_p95_latency_ms: u64,
    pub min_throughput_rps: f64,
    pub max_error_rate: f64,
    pub max_memory_usage_mb: f64,
}

impl PerformanceValidator {
    pub async fn validate_performance(&self) -> Result<PerformanceValidationResult, ValidationError> {
        let mut results = PerformanceValidationResult::default();

        // Test command latency
        let latency_test = self.test_command_latency().await?;
        results.latency_passed = latency_test.p95_latency_ms <= self.target_metrics.max_p95_latency_ms;

        // Test throughput
        let throughput_test = self.test_throughput().await?;
        results.throughput_passed = throughput_test.requests_per_second >= self.target_metrics.min_throughput_rps;

        // Test error rate
        let error_test = self.test_error_rate().await?;
        results.error_rate_passed = error_test.error_rate <= self.target_metrics.max_error_rate;

        // Test memory usage
        let memory_test = self.test_memory_usage().await?;
        results.memory_passed = memory_test.peak_memory_mb <= self.target_metrics.max_memory_usage_mb;

        results.overall_passed = results.latency_passed &&
                                 results.throughput_passed &&
                                 results.error_rate_passed &&
                                 results.memory_passed;

        Ok(results)
    }

    async fn test_command_latency(&self) -> Result<LatencyTestResult, ValidationError> {
        // Implement latency testing
        // Execute sample commands and measure response times
        Ok(LatencyTestResult {
            p95_latency_ms: 50, // Example result
            avg_latency_ms: 25,
        })
    }

    async fn test_throughput(&self) -> Result<ThroughputTestResult, ValidationError> {
        // Implement throughput testing
        // Execute concurrent commands and measure RPS
        Ok(ThroughputTestResult {
            requests_per_second: 150.0, // Example result
            peak_concurrent_requests: 50,
        })
    }
}

#[derive(Debug, Default)]
pub struct PerformanceValidationResult {
    pub latency_passed: bool,
    pub throughput_passed: bool,
    pub error_rate_passed: bool,
    pub memory_passed: bool,
    pub overall_passed: bool,
}
```

### Reliability

#### High Availability

- [ ] **Multiple replicas** deployed
- [ ] **Pod disruption budgets** configured
- [ ] **Health checks** implemented and tested
- [ ] **Readiness probes** properly configured
- [ ] **Liveness probes** tuned appropriately
- [ ] **Rolling update strategy** configured

```yaml
# High availability configuration
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: eventcore-pdb
  namespace: eventcore
spec:
  minAvailable: 2
  selector:
    matchLabels:
      app: eventcore
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: eventcore-app
spec:
  replicas: 3
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 1
      maxSurge: 1
  template:
    spec:
      containers:
        - name: eventcore-app
          readinessProbe:
            httpGet:
              path: /ready
              port: 8080
            initialDelaySeconds: 10
            periodSeconds: 5
            timeoutSeconds: 3
            failureThreshold: 3
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 60
            periodSeconds: 30
            timeoutSeconds: 5
            failureThreshold: 3
```

#### Backup and Recovery

- [ ] **Automated backups** configured and tested
- [ ] **Backup verification** automated
- [ ] **Recovery procedures** documented and tested
- [ ] **Point-in-time recovery** capability verified
- [ ] **Cross-region backup** replication configured
- [ ] **Backup retention** policies implemented

```rust
// Backup validation
pub struct BackupValidator;

impl BackupValidator {
    pub async fn validate_backup_system(&self) -> Result<BackupValidationResult, ValidationError> {
        let mut result = BackupValidationResult::default();

        // Test backup creation
        result.backup_creation = self.test_backup_creation().await?;

        // Test backup verification
        result.backup_verification = self.test_backup_verification().await?;

        // Test restore functionality
        result.restore_capability = self.test_restore_capability().await?;

        // Test backup schedule
        result.backup_schedule = self.verify_backup_schedule().await?;

        // Test retention policy
        result.retention_policy = self.verify_retention_policy().await?;

        result.overall_passed = result.backup_creation &&
                                result.backup_verification &&
                                result.restore_capability &&
                                result.backup_schedule &&
                                result.retention_policy;

        Ok(result)
    }
}

#[derive(Debug, Default)]
pub struct BackupValidationResult {
    pub backup_creation: bool,
    pub backup_verification: bool,
    pub restore_capability: bool,
    pub backup_schedule: bool,
    pub retention_policy: bool,
    pub overall_passed: bool,
}
```

### Monitoring and Observability

#### Metrics Collection

- [ ] **Application metrics** exported to Prometheus
- [ ] **Business metrics** tracked
- [ ] **Infrastructure metrics** monitored
- [ ] **Custom dashboards** created for key metrics
- [ ] **SLI/SLO** defined and monitored

```rust
// Metrics validation
pub struct MetricsValidator {
    prometheus_client: PrometheusClient,
}

impl MetricsValidator {
    pub async fn validate_metrics(&self) -> Result<MetricsValidationResult, ValidationError> {
        let mut result = MetricsValidationResult::default();

        // Check core application metrics
        result.core_metrics = self.check_core_metrics().await?;

        // Check business metrics
        result.business_metrics = self.check_business_metrics().await?;

        // Check infrastructure metrics
        result.infrastructure_metrics = self.check_infrastructure_metrics().await?;

        // Verify metric freshness
        result.metrics_current = self.check_metrics_freshness().await?;

        result.overall_passed = result.core_metrics &&
                                result.business_metrics &&
                                result.infrastructure_metrics &&
                                result.metrics_current;

        Ok(result)
    }

    async fn check_core_metrics(&self) -> Result<bool, ValidationError> {
        let required_metrics = vec![
            "eventcore_commands_total",
            "eventcore_command_duration_seconds",
            "eventcore_events_written_total",
            "eventcore_active_streams",
            "eventcore_projection_lag_seconds",
        ];

        for metric in required_metrics {
            if !self.prometheus_client.metric_exists(metric).await? {
                return Ok(false);
            }
        }

        Ok(true)
    }
}
```

#### Logging

- [ ] **Structured logging** implemented
- [ ] **Log aggregation** configured
- [ ] **Log retention** policies set
- [ ] **Correlation IDs** used throughout
- [ ] **Log levels** appropriately configured
- [ ] **Sensitive data** excluded from logs

#### Alerting

- [ ] **Critical alerts** configured
- [ ] **Warning alerts** tuned to reduce noise
- [ ] **Alert routing** configured for different severities
- [ ] **Escalation policies** defined
- [ ] **Alert fatigue** minimized through proper thresholds

```yaml
# Alerting validation checklist
groups:
  - name: eventcore-critical
    rules:
      - alert: EventCoreDown
        expr: up{job="eventcore"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "EventCore service is down"

      - alert: HighErrorRate
        expr: rate(eventcore_command_errors_total[5m]) / rate(eventcore_commands_total[5m]) > 0.05
        for: 3m
        labels:
          severity: critical
        annotations:
          summary: "High error rate detected"

      - alert: DatabaseConnectionFailure
        expr: eventcore_connection_pool_errors_total > 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Database connection issues"
```

## Deployment Checklist

### Environment Configuration

- [ ] **Environment variables** properly set
- [ ] **Secrets** configured and mounted
- [ ] **Config maps** updated
- [ ] **Feature flags** configured appropriately
- [ ] **Resource limits** applied
- [ ] **Network policies** configured

### Database Setup

- [ ] **Database migrations** applied and verified
- [ ] **Database indexes** created and optimized
- [ ] **Database monitoring** configured
- [ ] **Connection pooling** tuned
- [ ] **Backup strategy** implemented
- [ ] **Read replicas** configured if needed

### Infrastructure

- [ ] **DNS records** configured
- [ ] **Load balancer** configured
- [ ] **SSL certificates** installed and valid
- [ ] **CDN** configured if applicable
- [ ] **Firewall rules** applied
- [ ] **Network segmentation** implemented

## Post-Deployment Verification

### Functional Testing

- [ ] **Smoke tests** pass
- [ ] **Critical user journeys** work
- [ ] **API endpoints** respond correctly
- [ ] **Authentication** works
- [ ] **Authorization** enforced
- [ ] **Error handling** works properly

```rust
// Post-deployment validation suite
pub struct PostDeploymentValidator {
    base_url: String,
    auth_token: String,
}

impl PostDeploymentValidator {
    pub async fn run_validation_suite(&self) -> Result<ValidationSuite, ValidationError> {
        let mut suite = ValidationSuite::default();

        // Test 1: Health check
        suite.health_check = self.test_health_endpoint().await?;

        // Test 2: Authentication
        suite.authentication = self.test_authentication().await?;

        // Test 3: Core functionality
        suite.core_functionality = self.test_core_functionality().await?;

        // Test 4: Performance
        suite.performance = self.test_basic_performance().await?;

        // Test 5: Error handling
        suite.error_handling = self.test_error_handling().await?;

        suite.overall_passed = suite.health_check &&
                               suite.authentication &&
                               suite.core_functionality &&
                               suite.performance &&
                               suite.error_handling;

        Ok(suite)
    }

    async fn test_health_endpoint(&self) -> Result<bool, ValidationError> {
        let response = reqwest::get(&format!("{}/health", self.base_url)).await?;
        Ok(response.status().is_success())
    }

    async fn test_authentication(&self) -> Result<bool, ValidationError> {
        // Test with valid token
        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/api/v1/test", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(false);
        }

        // Test without token (should fail)
        let response = client
            .get(&format!("{}/api/v1/test", self.base_url))
            .send()
            .await?;

        Ok(response.status() == 401)
    }

    async fn test_core_functionality(&self) -> Result<bool, ValidationError> {
        // Test a simple command execution
        let client = reqwest::Client::new();
        let create_user_payload = serde_json::json!({
            "email": "test@example.com",
            "first_name": "Test",
            "last_name": "User"
        });

        let response = client
            .post(&format!("{}/api/v1/users", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&create_user_payload)
            .send()
            .await?;

        Ok(response.status().is_success())
    }
}

#[derive(Debug, Default)]
pub struct ValidationSuite {
    pub health_check: bool,
    pub authentication: bool,
    pub core_functionality: bool,
    pub performance: bool,
    pub error_handling: bool,
    pub overall_passed: bool,
}
```

### Performance Validation

- [ ] **Response times** within acceptable limits
- [ ] **Throughput** meets requirements
- [ ] **Resource usage** within limits
- [ ] **Memory leaks** not detected
- [ ] **CPU usage** stable
- [ ] **Database performance** optimal

### Monitoring Validation

- [ ] **Metrics** flowing to monitoring system
- [ ] **Logs** being collected and indexed
- [ ] **Traces** visible in tracing system
- [ ] **Alerts** triggering appropriately
- [ ] **Dashboards** showing correct data
- [ ] **SLI/SLO** monitoring active

## Ongoing Operations Checklist

### Daily Checks

- [ ] **System health** green across all services
- [ ] **Error rates** within acceptable thresholds
- [ ] **Performance metrics** meeting SLOs
- [ ] **Resource utilization** not approaching limits
- [ ] **Log analysis** for new error patterns
- [ ] **Security alerts** reviewed

### Weekly Checks

- [ ] **Backup verification** completed successfully
- [ ] **Performance trends** analyzed
- [ ] **Capacity planning** reviewed
- [ ] **Security patches** evaluated and applied
- [ ] **Dependency updates** reviewed
- [ ] **Documentation** updated as needed

### Monthly Checks

- [ ] **Disaster recovery** procedures tested
- [ ] **Security audit** completed
- [ ] **Performance benchmarks** updated
- [ ] **Cost optimization** opportunities identified
- [ ] **Capacity forecasting** updated
- [ ] **Runbook accuracy** verified

## Automation Scripts

### Deployment Validation Script

```bash
#!/bin/bash
# deployment-validation.sh

set -e

NAMESPACE="eventcore"
APP_NAME="eventcore-app"
BASE_URL="https://api.eventcore.example.com"

echo "🚀 Starting deployment validation..."

# Check deployment status
echo "📋 Checking deployment status..."
kubectl rollout status deployment/$APP_NAME -n $NAMESPACE --timeout=300s

# Check pod health
echo "🏥 Checking pod health..."
READY_PODS=$(kubectl get pods -l app=$APP_NAME -n $NAMESPACE -o jsonpath='{.items[?(@.status.phase=="Running")].metadata.name}' | wc -w)
DESIRED_PODS=$(kubectl get deployment $APP_NAME -n $NAMESPACE -o jsonpath='{.spec.replicas}')

if [ "$READY_PODS" -ne "$DESIRED_PODS" ]; then
    echo "❌ Not all pods are ready: $READY_PODS/$DESIRED_PODS"
    exit 1
fi

echo "✅ All pods are ready: $READY_PODS/$DESIRED_PODS"

# Check health endpoint
echo "🔍 Testing health endpoint..."
HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" $BASE_URL/health)
if [ "$HTTP_STATUS" -ne 200 ]; then
    echo "❌ Health check failed with status: $HTTP_STATUS"
    exit 1
fi

echo "✅ Health check passed"

# Check metrics endpoint
echo "📊 Testing metrics endpoint..."
HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" $BASE_URL/metrics)
if [ "$HTTP_STATUS" -ne 200 ]; then
    echo "❌ Metrics endpoint failed with status: $HTTP_STATUS"
    exit 1
fi

echo "✅ Metrics endpoint responding"

# Check database connectivity
echo "🗄️ Testing database connectivity..."
kubectl exec -n $NAMESPACE deployment/$APP_NAME -- eventcore-cli health-check database
if [ $? -ne 0 ]; then
    echo "❌ Database connectivity check failed"
    exit 1
fi

echo "✅ Database connectivity verified"

# Run smoke tests
echo "💨 Running smoke tests..."
kubectl exec -n $NAMESPACE deployment/$APP_NAME -- eventcore-cli test smoke
if [ $? -ne 0 ]; then
    echo "❌ Smoke tests failed"
    exit 1
fi

echo "✅ Smoke tests passed"

echo "🎉 Deployment validation completed successfully!"
```

### Health Check Script

```bash
#!/bin/bash
# health-check.sh

set -e

NAMESPACE="eventcore"
PROMETHEUS_URL="http://prometheus.monitoring.svc.cluster.local:9090"

echo "🔍 Running comprehensive health check..."

# Check application health
echo "📱 Checking application health..."
APP_UP=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=up{job=\"eventcore\"}" | jq '.data.result[0].value[1]' -r)
if [ "$APP_UP" != "1" ]; then
    echo "❌ Application is down"
    exit 1
fi

# Check error rate
echo "🚨 Checking error rate..."
ERROR_RATE=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=rate(eventcore_command_errors_total[5m])/rate(eventcore_commands_total[5m])" | jq '.data.result[0].value[1]' -r)
if (( $(echo "$ERROR_RATE > 0.05" | bc -l) )); then
    echo "❌ High error rate detected: $ERROR_RATE"
    exit 1
fi

# Check response time
echo "⏱️ Checking response time..."
P95_LATENCY=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=histogram_quantile(0.95, rate(eventcore_command_duration_seconds_bucket[5m]))" | jq '.data.result[0].value[1]' -r)
if (( $(echo "$P95_LATENCY > 1.0" | bc -l) )); then
    echo "❌ High latency detected: ${P95_LATENCY}s"
    exit 1
fi

# Check database connectivity
echo "🗄️ Checking database health..."
DB_CONNECTIONS=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=eventcore_connection_pool_size" | jq '.data.result[0].value[1]' -r)
MAX_CONNECTIONS=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=eventcore_connection_pool_max_size" | jq '.data.result[0].value[1]' -r)
UTILIZATION=$(echo "scale=2; $DB_CONNECTIONS / $MAX_CONNECTIONS" | bc)

if (( $(echo "$UTILIZATION > 0.8" | bc -l) )); then
    echo "⚠️ High database connection utilization: $UTILIZATION"
fi

echo "✅ All health checks passed!"
```

## Emergency Procedures

### Incident Response

1. **Assess severity** using incident severity matrix
2. **Activate incident response team** if critical
3. **Create incident tracking** (ticket/channel)
4. **Implement immediate mitigation** if possible
5. **Communicate status** to stakeholders
6. **Investigate root cause** after mitigation
7. **Document lessons learned** and improvements

### Rollback Procedures

```bash
#!/bin/bash
# emergency-rollback.sh

NAMESPACE="eventcore"
APP_NAME="eventcore-app"

echo "🚨 Emergency rollback initiated..."

# Get previous revision
CURRENT_REVISION=$(kubectl rollout history deployment/$APP_NAME -n $NAMESPACE --output=json | jq '.items[-1].revision')
PREVIOUS_REVISION=$((CURRENT_REVISION - 1))

echo "Rolling back from revision $CURRENT_REVISION to $PREVIOUS_REVISION"

# Perform rollback
kubectl rollout undo deployment/$APP_NAME -n $NAMESPACE --to-revision=$PREVIOUS_REVISION

# Wait for rollback to complete
kubectl rollout status deployment/$APP_NAME -n $NAMESPACE --timeout=300s

# Verify health
sleep 30
./health-check.sh

echo "✅ Emergency rollback completed"
```

## Summary

Production readiness checklist for EventCore:

- ✅ **Security** - Authentication, authorization, encryption
- ✅ **Performance** - Resource limits, optimization, benchmarks
- ✅ **Reliability** - High availability, backup and recovery
- ✅ **Monitoring** - Metrics, logging, alerting, dashboards
- ✅ **Operations** - Deployment validation, health checks, incident response

Key principles:

1. **Validate everything** - Don't assume anything works in production
2. **Automate checks** - Use scripts and tools for consistent validation
3. **Monitor continuously** - Track all critical metrics and logs
4. **Plan for failure** - Have rollback and recovery procedures ready
5. **Document procedures** - Maintain up-to-date runbooks and checklists

This completes the EventCore Operations guide. You now have comprehensive documentation for deploying, monitoring, and maintaining EventCore applications in production environments.

Next, proceed to [Part 7: Reference](../07-reference/README.md) →
