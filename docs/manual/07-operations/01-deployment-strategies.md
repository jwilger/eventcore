# Chapter 6.1: Deployment Strategies

EventCore applications require careful deployment planning to ensure high availability, data consistency, and smooth rollouts. This chapter covers production-ready deployment patterns and strategies.

## Container-Based Deployment

### Docker Configuration

EventCore applications containerize well with proper configuration:

```dockerfile
# Multi-stage build for optimized production image
FROM rust:1.87-slim as builder

WORKDIR /usr/src/app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build with release optimizations
RUN cargo build --release --locked

# Runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false eventcore

# Copy application
COPY --from=builder /usr/src/app/target/release/eventcore-app /usr/local/bin/
RUN chmod +x /usr/local/bin/eventcore-app

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

USER eventcore
EXPOSE 8080

CMD ["eventcore-app"]
```

### Environment Configuration

Use environment variables for configuration:

```bash
# Database configuration
DATABASE_URL=postgresql://user:pass@db:5432/eventcore
DATABASE_MAX_CONNECTIONS=20
DATABASE_ACQUIRE_TIMEOUT=30s

# Application configuration
HTTP_PORT=8080
LOG_LEVEL=info
LOG_FORMAT=json

# Performance tuning
COMMAND_TIMEOUT=30s
EVENT_BATCH_SIZE=100
PROJECTION_WORKERS=4

# Security
JWT_SECRET_KEY=/run/secrets/jwt_key
CORS_ALLOWED_ORIGINS=https://myapp.com

# Monitoring
METRICS_PORT=9090
TRACING_ENDPOINT=http://jaeger:14268/api/traces
HEALTH_CHECK_INTERVAL=30s
```

### Docker Compose for Development

```yaml
version: "3.8"

services:
  eventcore-app:
    build: .
    ports:
      - "8080:8080"
      - "9090:9090"
    environment:
      DATABASE_URL: postgresql://postgres:password@postgres:5432/eventcore
      LOG_LEVEL: debug
      METRICS_PORT: 9090
    depends_on:
      postgres:
        condition: service_healthy
    networks:
      - eventcore
    restart: unless-stopped

  postgres:
    image: postgres:17-alpine
    environment:
      POSTGRES_DB: eventcore
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: password
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./migrations:/docker-entrypoint-initdb.d
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks:
      - eventcore

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9091:9090"
    volumes:
      - ./config/prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus_data:/prometheus
    command:
      - "--config.file=/etc/prometheus/prometheus.yml"
      - "--storage.tsdb.path=/prometheus"
      - "--web.console.libraries=/etc/prometheus/console_libraries"
      - "--web.console.templates=/etc/prometheus/consoles"
      - "--web.enable-lifecycle"
    networks:
      - eventcore

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      GF_SECURITY_ADMIN_PASSWORD: admin
    volumes:
      - grafana_data:/var/lib/grafana
      - ./config/grafana:/etc/grafana/provisioning
    networks:
      - eventcore

volumes:
  postgres_data:
  prometheus_data:
  grafana_data:

networks:
  eventcore:
    driver: bridge
```

## Kubernetes Deployment

### Application Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: eventcore-app
  namespace: eventcore
  labels:
    app: eventcore
    component: application
spec:
  replicas: 3
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 1
      maxSurge: 1
  selector:
    matchLabels:
      app: eventcore
      component: application
  template:
    metadata:
      labels:
        app: eventcore
        component: application
    spec:
      serviceAccountName: eventcore
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        fsGroup: 1000
      containers:
        - name: eventcore-app
          image: eventcore:latest
          imagePullPolicy: Always
          ports:
            - containerPort: 8080
              name: http
            - containerPort: 9090
              name: metrics
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: eventcore-secrets
                  key: database-url
            - name: JWT_SECRET_KEY
              valueFrom:
                secretKeyRef:
                  name: eventcore-secrets
                  key: jwt-secret
          envFrom:
            - configMapRef:
                name: eventcore-config
          resources:
            requests:
              memory: "256Mi"
              cpu: "250m"
            limits:
              memory: "512Mi"
              cpu: "500m"
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 30
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 3
          readinessProbe:
            httpGet:
              path: /ready
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 5
            timeoutSeconds: 3
            failureThreshold: 3
          volumeMounts:
            - name: config
              mountPath: /etc/eventcore
              readOnly: true
      volumes:
        - name: config
          configMap:
            name: eventcore-config
---
apiVersion: v1
kind: Service
metadata:
  name: eventcore-service
  namespace: eventcore
  labels:
    app: eventcore
    component: application
spec:
  type: ClusterIP
  ports:
    - port: 80
      targetPort: 8080
      protocol: TCP
      name: http
    - port: 9090
      targetPort: 9090
      protocol: TCP
      name: metrics
  selector:
    app: eventcore
    component: application
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: eventcore-config
  namespace: eventcore
data:
  HTTP_PORT: "8080"
  METRICS_PORT: "9090"
  LOG_LEVEL: "info"
  LOG_FORMAT: "json"
  COMMAND_TIMEOUT: "30s"
  EVENT_BATCH_SIZE: "100"
  PROJECTION_WORKERS: "4"
  HEALTH_CHECK_INTERVAL: "30s"
---
apiVersion: v1
kind: Secret
metadata:
  name: eventcore-secrets
  namespace: eventcore
type: Opaque
data:
  database-url: <base64-encoded-database-url>
  jwt-secret: <base64-encoded-jwt-secret>
```

### Database Configuration

```yaml
apiVersion: postgresql.cnpg.io/v1
kind: Cluster
metadata:
  name: postgres-cluster
  namespace: eventcore
spec:
  instances: 3
  primaryUpdateStrategy: unsupervised

  postgresql:
    parameters:
      max_connections: "200"
      shared_buffers: "256MB"
      effective_cache_size: "1GB"
      maintenance_work_mem: "64MB"
      checkpoint_completion_target: "0.9"
      wal_buffers: "16MB"
      default_statistics_target: "100"
      random_page_cost: "1.1"
      effective_io_concurrency: "200"

  bootstrap:
    initdb:
      database: eventcore
      owner: eventcore
      secret:
        name: postgres-credentials

  storage:
    size: 100Gi
    storageClass: fast-ssd

  monitoring:
    enabled: true

  backup:
    target: prefer-standby
    retentionPolicy: "30d"
    data:
      compression: gzip
      encryption: AES256
      jobs: 2
    wal:
      compression: gzip
      encryption: AES256
---
apiVersion: v1
kind: Secret
metadata:
  name: postgres-credentials
  namespace: eventcore
type: kubernetes.io/basic-auth
data:
  username: <base64-encoded-username>
  password: <base64-encoded-password>
```

### Ingress Configuration

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: eventcore-ingress
  namespace: eventcore
  annotations:
    kubernetes.io/ingress.class: nginx
    cert-manager.io/cluster-issuer: letsencrypt-prod
    nginx.ingress.kubernetes.io/ssl-redirect: "true"
    nginx.ingress.kubernetes.io/force-ssl-redirect: "true"
    nginx.ingress.kubernetes.io/rate-limit: "100"
    nginx.ingress.kubernetes.io/rate-limit-window: "1m"
spec:
  tls:
    - hosts:
        - api.eventcore.example.com
      secretName: eventcore-tls
  rules:
    - host: api.eventcore.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: eventcore-service
                port:
                  number: 80
```

## Blue-Green Deployment

### Deployment Strategy

Blue-green deployment ensures zero-downtime updates:

```yaml
# Blue environment (current production)
apiVersion: apps/v1
kind: Deployment
metadata:
  name: eventcore-blue
  namespace: eventcore
  labels:
    app: eventcore
    environment: blue
spec:
  replicas: 3
  selector:
    matchLabels:
      app: eventcore
      environment: blue
  template:
    metadata:
      labels:
        app: eventcore
        environment: blue
    spec:
      containers:
        - name: eventcore-app
          image: eventcore:v1.0.0
          # ... container spec
---
# Green environment (new version)
apiVersion: apps/v1
kind: Deployment
metadata:
  name: eventcore-green
  namespace: eventcore
  labels:
    app: eventcore
    environment: green
spec:
  replicas: 3
  selector:
    matchLabels:
      app: eventcore
      environment: green
  template:
    metadata:
      labels:
        app: eventcore
        environment: green
    spec:
      containers:
        - name: eventcore-app
          image: eventcore:v1.1.0
          # ... container spec
---
# Service that can switch between environments
apiVersion: v1
kind: Service
metadata:
  name: eventcore-service
  namespace: eventcore
spec:
  selector:
    app: eventcore
    environment: blue # Switch to 'green' when deploying
  ports:
    - port: 80
      targetPort: 8080
```

### Deployment Script

```bash
#!/bin/bash
set -e

NAMESPACE="eventcore"
NEW_VERSION="$1"
CURRENT_ENV="blue"
TARGET_ENV="green"

if [[ -z "$NEW_VERSION" ]]; then
    echo "Usage: $0 <new-version>"
    exit 1
fi

echo "Starting blue-green deployment to version $NEW_VERSION"

# Get current environment
CURRENT_SELECTOR=$(kubectl get service eventcore-service -n $NAMESPACE -o jsonpath='{.spec.selector.environment}')
if [[ "$CURRENT_SELECTOR" == "blue" ]]; then
    TARGET_ENV="green"
    CURRENT_ENV="blue"
else
    TARGET_ENV="blue"
    CURRENT_ENV="green"
fi

echo "Current environment: $CURRENT_ENV"
echo "Target environment: $TARGET_ENV"

# Update target environment with new version
kubectl set image deployment/eventcore-$TARGET_ENV -n $NAMESPACE \
    eventcore-app=eventcore:$NEW_VERSION

# Wait for rollout to complete
kubectl rollout status deployment/eventcore-$TARGET_ENV -n $NAMESPACE

# Health check on target environment
echo "Performing health checks..."
TARGET_POD=$(kubectl get pods -n $NAMESPACE -l environment=$TARGET_ENV -o jsonpath='{.items[0].metadata.name}')
kubectl exec -n $NAMESPACE $TARGET_POD -- curl -f http://localhost:8080/health

# Run smoke tests
echo "Running smoke tests..."
kubectl port-forward -n $NAMESPACE service/eventcore-$TARGET_ENV 8081:80 &
PORT_FORWARD_PID=$!
sleep 5

# Basic functionality test
curl -f http://localhost:8081/health
curl -f http://localhost:8081/metrics

kill $PORT_FORWARD_PID

# Switch traffic to target environment
echo "Switching traffic to $TARGET_ENV environment"
kubectl patch service eventcore-service -n $NAMESPACE \
    -p '{"spec":{"selector":{"environment":"'$TARGET_ENV'"}}}'

echo "Deployment complete. Traffic switched to $TARGET_ENV"
echo "Old environment ($CURRENT_ENV) is still running for rollback if needed"
echo "To rollback: kubectl patch service eventcore-service -n $NAMESPACE -p '{\"spec\":{\"selector\":{\"environment\":\"$CURRENT_ENV\"}}}'"
```

## Canary Deployment

### Traffic Splitting with Istio

```yaml
apiVersion: networking.istio.io/v1beta1
kind: VirtualService
metadata:
  name: eventcore-canary
  namespace: eventcore
spec:
  hosts:
    - api.eventcore.example.com
  http:
    - match:
        - headers:
            canary:
              exact: "true"
      route:
        - destination:
            host: eventcore-service
            subset: canary
    - route:
        - destination:
            host: eventcore-service
            subset: stable
          weight: 95
        - destination:
            host: eventcore-service
            subset: canary
          weight: 5
---
apiVersion: networking.istio.io/v1beta1
kind: DestinationRule
metadata:
  name: eventcore-destination
  namespace: eventcore
spec:
  host: eventcore-service
  subsets:
    - name: stable
      labels:
        version: stable
    - name: canary
      labels:
        version: canary
```

### Automated Canary with Flagger

```yaml
apiVersion: flagger.app/v1beta1
kind: Canary
metadata:
  name: eventcore
  namespace: eventcore
spec:
  targetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: eventcore
  progressDeadlineSeconds: 60
  service:
    port: 80
    targetPort: 8080
  analysis:
    interval: 1m
    threshold: 5
    maxWeight: 50
    stepWeight: 10
    metrics:
      - name: request-success-rate
        thresholdRange:
          min: 99
        interval: 1m
      - name: request-duration
        thresholdRange:
          max: 500
        interval: 30s
    webhooks:
      - name: smoke-test
        type: pre-rollout
        url: http://flagger-loadtester.test/
        timeout: 15s
        metadata:
          type: bash
          cmd: "curl -sd 'test' http://eventcore-canary/health"
      - name: load-test
        url: http://flagger-loadtester.test/
        timeout: 5s
        metadata:
          cmd: "hey -z 1m -q 10 -c 2 http://eventcore-canary/"
```

## Database Migrations

### Schema Migration Strategy

```rust
use sqlx::{PgPool, migrate::MigrateDatabase, Postgres};

pub struct MigrationManager {
    pool: PgPool,
    migration_path: String,
}

impl MigrationManager {
    pub async fn new(database_url: &str, migration_path: String) -> Result<Self, sqlx::Error> {
        // Ensure database exists
        if !Postgres::database_exists(database_url).await? {
            Postgres::create_database(database_url).await?;
        }

        let pool = PgPool::connect(database_url).await?;

        Ok(Self {
            pool,
            migration_path,
        })
    }

    pub async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        sqlx::migrate::Migrator::new(std::path::Path::new(&self.migration_path))
            .await?
            .run(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn check_migration_status(&self) -> Result<MigrationStatus, sqlx::Error> {
        let migrator = sqlx::migrate::Migrator::new(std::path::Path::new(&self.migration_path))
            .await?;

        let applied = migrator.get_applied_migrations(&self.pool).await?;
        let available = migrator.iter().count();

        Ok(MigrationStatus {
            applied: applied.len(),
            available,
            pending: available - applied.len(),
        })
    }
}

#[derive(Debug)]
pub struct MigrationStatus {
    pub applied: usize,
    pub available: usize,
    pub pending: usize,
}
```

### Migration Files Structure

```
migrations/
├── 001_initial_schema.sql
├── 002_add_user_preferences.sql
├── 003_optimize_event_indexes.sql
└── 004_add_projection_checkpoints.sql
```

#### Metadata Column Upgrade (eventcore-postgres)

EventCore 0.1 introduced the `metadata` column to persist per-event envelopes. Existing
deployments that created the `eventcore_events` table before this change can roll out the
new column without downtime by running the additive migration `0002_add_metadata_column.sql`
shipped with the `eventcore-postgres` crate.

**Recommended rollout procedure**

1. **Back up** the production database (logical or physical snapshot) before running migrations.
2. **Apply the migration** ahead of the application deploy:

   ```bash
   DATABASE_URL=postgres://postgres:password@db:5432/eventcore \
     sqlx migrate run --source eventcore-postgres/migrations
   ```

   Migration `0002_add_metadata_column.sql` executes the following statement:

   ```sql
   ALTER TABLE eventcore_events
       ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '{}'::jsonb;
   ```

3. **Verify** that existing rows were back-filled with an empty JSON payload:

   ```sql
   SELECT DISTINCT metadata FROM eventcore_events;
   ```

   The result should be a single row containing `{}` until the new code starts storing
   richer metadata.

4. **Deploy** the new application version. Because the column addition is additive and the
   default keeps existing rows readable, this can be done with the usual rolling/blue-green
   strategy.

5. **Monitor** the new telemetry (`postgres.version_conflict` logs and structured metadata)
   to confirm the migration succeeded.

The regression test `eventcore-postgres/tests/i014_schema_migration_test.rs` exercises this
workflow by installing the legacy schema, running `PostgresEventStore::migrate()`, and then
appending new events to ensure backward compatibility. Use it as a template for future
additive migrations.


Example migration:

```sql
-- migrations/001_initial_schema.sql
-- Create events table with optimized indexes
CREATE TABLE events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    stream_id VARCHAR(255) NOT NULL,
    version BIGINT NOT NULL,
    event_type VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    occurred_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),

    CONSTRAINT events_stream_version_unique UNIQUE (stream_id, version)
);

-- Optimized indexes for common query patterns
CREATE INDEX idx_events_stream_id ON events (stream_id);
CREATE INDEX idx_events_stream_id_version ON events (stream_id, version);
CREATE INDEX idx_events_occurred_at ON events (occurred_at);
CREATE INDEX idx_events_event_type ON events (event_type);
CREATE INDEX idx_events_payload_gin ON events USING GIN (payload);

-- Create projection checkpoints table
CREATE TABLE projection_checkpoints (
    projection_name VARCHAR(255) PRIMARY KEY,
    last_event_id UUID,
    last_event_version BIGINT,
    stream_positions JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_projection_checkpoints_updated_at ON projection_checkpoints (updated_at);
```

### Zero-Downtime Migration Pattern

```bash
#!/bin/bash
# Zero-downtime migration script

set -e

DATABASE_URL="$1"
MIGRATION_PATH="./migrations"

echo "Starting zero-downtime migration process..."

# Step 1: Run additive migrations (safe)
echo "Running additive migrations..."
sqlx migrate run --source $MIGRATION_PATH/additive

# Step 2: Deploy new application version (backward compatible)
echo "Deploying new application version..."
kubectl set image deployment/eventcore-app eventcore-app=eventcore:$NEW_VERSION
kubectl rollout status deployment/eventcore-app

# Step 3: Verify application health
echo "Verifying application health..."
kubectl get pods -l app=eventcore
curl -f http://api.eventcore.example.com/health

# Step 4: Run data migrations (if needed)
echo "Running data migrations..."
sqlx migrate run --source $MIGRATION_PATH/data

# Step 5: Run cleanup migrations (remove old columns/tables)
echo "Running cleanup migrations..."
sqlx migrate run --source $MIGRATION_PATH/cleanup

echo "Zero-downtime migration completed successfully!"
```

## Configuration Management

### Environment-Specific Configuration

```rust
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub monitoring: MonitoringConfig,
    pub features: FeatureFlags,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub acquire_timeout_seconds: u64,
    pub command_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
    pub request_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitoringConfig {
    pub metrics_port: u16,
    pub tracing_endpoint: Option<String>,
    pub log_level: String,
    pub health_check_interval_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FeatureFlags {
    pub enable_metrics: bool,
    pub enable_tracing: bool,
    pub enable_auth: bool,
    pub enable_rate_limiting: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

        let config = Config::builder()
            // Start with default configuration
            .add_source(File::with_name("config/default"))
            // Add environment-specific configuration
            .add_source(File::with_name(&format!("config/{}", environment)).required(false))
            // Add local configuration (for development)
            .add_source(File::with_name("config/local").required(false))
            // Override with environment variables
            .add_source(Environment::with_prefix("EVENTCORE").separator("_"))
            .build()?;

        config.try_deserialize()
    }
}
```

### Configuration Files

```yaml
# config/default.yaml
database:
  max_connections: 10
  acquire_timeout_seconds: 30
  command_timeout_seconds: 60

server:
  host: "0.0.0.0"
  port: 8080
  cors_origins: ["http://localhost:3000"]
  request_timeout_seconds: 30

monitoring:
  metrics_port: 9090
  log_level: "info"
  health_check_interval_seconds: 30

features:
  enable_metrics: true
  enable_tracing: false
  enable_auth: false
  enable_rate_limiting: false
```

```yaml
# config/production.yaml
database:
  max_connections: 20
  acquire_timeout_seconds: 10
  command_timeout_seconds: 30

server:
  cors_origins: ["https://myapp.com"]
  request_timeout_seconds: 15

monitoring:
  log_level: "warn"
  health_check_interval_seconds: 10

features:
  enable_tracing: true
  enable_auth: true
  enable_rate_limiting: true
```

## Health Checks and Readiness

### Application Health Endpoints

```rust
use axum::{Json, response::Json as JsonResponse, extract::State};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct HealthService {
    event_store: Arc<dyn EventStore>,
    dependencies: Vec<Arc<dyn HealthCheck>>,
}

#[async_trait]
pub trait HealthCheck: Send + Sync {
    async fn name(&self) -> &'static str;
    async fn check(&self) -> HealthStatus;
}

#[derive(Debug, Clone)]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
    Unknown,
}

impl HealthService {
    pub async fn health_check(&self) -> JsonResponse<Value> {
        let mut overall_healthy = true;
        let mut checks = Vec::new();

        // Check event store
        let event_store_status = self.check_event_store().await;
        let event_store_healthy = matches!(event_store_status, HealthStatus::Healthy);
        overall_healthy &= event_store_healthy;

        checks.push(json!({
            "name": "event_store",
            "status": if event_store_healthy { "healthy" } else { "unhealthy" },
            "details": match event_store_status {
                HealthStatus::Unhealthy(msg) => Some(msg),
                _ => None,
            }
        }));

        // Check dependencies
        for dependency in &self.dependencies {
            let name = dependency.name().await;
            let status = dependency.check().await;
            let healthy = matches!(status, HealthStatus::Healthy);
            overall_healthy &= healthy;

            checks.push(json!({
                "name": name,
                "status": if healthy { "healthy" } else { "unhealthy" },
                "details": match status {
                    HealthStatus::Unhealthy(msg) => Some(msg),
                    _ => None,
                }
            }));
        }

        let response = json!({
            "status": if overall_healthy { "healthy" } else { "unhealthy" },
            "checks": checks,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "version": env!("CARGO_PKG_VERSION")
        });

        JsonResponse(response)
    }

    pub async fn readiness_check(&self) -> JsonResponse<Value> {
        // Readiness is stricter - all components must be ready
        let event_store_ready = self.check_event_store_ready().await;
        let migrations_ready = self.check_migrations_ready().await;

        let ready = event_store_ready && migrations_ready;

        let response = json!({
            "status": if ready { "ready" } else { "not_ready" },
            "checks": {
                "event_store": event_store_ready,
                "migrations": migrations_ready,
            },
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        JsonResponse(response)
    }

    async fn check_event_store(&self) -> HealthStatus {
        match self.event_store.health_check().await {
            Ok(_) => HealthStatus::Healthy,
            Err(e) => HealthStatus::Unhealthy(format!("Event store error: {}", e)),
        }
    }

    async fn check_event_store_ready(&self) -> bool {
        // More stringent check for readiness
        self.event_store.ping().await.is_ok()
    }

    async fn check_migrations_ready(&self) -> bool {
        // Check if all migrations are applied
        match self.event_store.migration_status().await {
            Ok(status) => status.pending == 0,
            Err(_) => false,
        }
    }
}

// Route handlers
pub async fn health_handler(State(health_service): State<HealthService>) -> JsonResponse<Value> {
    health_service.health_check().await
}

pub async fn readiness_handler(State(health_service): State<HealthService>) -> JsonResponse<Value> {
    health_service.readiness_check().await
}

pub async fn liveness_handler() -> JsonResponse<Value> {
    // Simple liveness check - just return OK if the process is running
    JsonResponse(json!({
        "status": "alive",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}
```

### Kubernetes Health Check Configuration

```yaml
# Detailed health check configuration
spec:
  containers:
    - name: eventcore-app
      # Liveness probe - restart container if this fails
      livenessProbe:
        httpGet:
          path: /liveness
          port: 8080
          httpHeaders:
            - name: Accept
              value: application/json
        initialDelaySeconds: 30
        periodSeconds: 30
        timeoutSeconds: 5
        failureThreshold: 3
        successThreshold: 1

      # Readiness probe - remove from service if this fails
      readinessProbe:
        httpGet:
          path: /readiness
          port: 8080
          httpHeaders:
            - name: Accept
              value: application/json
        initialDelaySeconds: 5
        periodSeconds: 10
        timeoutSeconds: 3
        failureThreshold: 3
        successThreshold: 1

      # Startup probe - give extra time during startup
      startupProbe:
        httpGet:
          path: /health
          port: 8080
        initialDelaySeconds: 10
        periodSeconds: 5
        timeoutSeconds: 3
        failureThreshold: 30
        successThreshold: 1
```

## Best Practices

1. **Containerize everything** - Use containers for consistent deployments
2. **Infrastructure as Code** - Version control all configuration
3. **Zero-downtime deployments** - Use blue-green or canary strategies
4. **Database migrations** - Plan for backward compatibility
5. **Health monitoring** - Implement comprehensive health checks
6. **Configuration management** - Separate config from code
7. **Security** - Use secrets management and RBAC
8. **Rollback plans** - Always have a rollback strategy

## Summary

EventCore deployment strategies:

- ✅ **Containerized** - Docker and Kubernetes ready
- ✅ **Zero-downtime** - Blue-green and canary deployments
- ✅ **Database migrations** - Safe schema evolution
- ✅ **Health monitoring** - Comprehensive health checks
- ✅ **Configuration management** - Environment-specific config

Key patterns:

1. Use containers for consistent, portable deployments
2. Implement blue-green or canary deployments for zero downtime
3. Plan database migrations for backward compatibility
4. Configure comprehensive health checks for reliability
5. Manage configuration separately from application code

Next, let's explore [Monitoring and Metrics](./02-monitoring-metrics.md) →
