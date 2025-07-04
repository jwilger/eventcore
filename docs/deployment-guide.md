# EventCore Deployment Guide

This guide covers deployment strategies and best practices for running EventCore-based applications in production environments. EventCore is a library that's integrated into your application, so deployment focuses on properly configuring and running your application that uses EventCore.

## Prerequisites

- Rust application built with EventCore
- PostgreSQL database (if using PostgreSQL event store)
- Container runtime (for containerized deployments)
- Basic understanding of event sourcing concepts

## Environment Configuration

### Required Environment Variables

```bash
# Database Configuration
DATABASE_URL=postgres://user:password@host:5432/eventcore
DATABASE_POOL_SIZE=20
DATABASE_CONNECTION_TIMEOUT=30s

# Application Configuration
RUST_LOG=info,eventcore=debug
APP_ENV=production
APP_PORT=8080

# Monitoring (if enabled)
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
PROMETHEUS_ENDPOINT=0.0.0.0:9090
```

### Configuration Best Practices

1. **Use environment-specific configs**: Separate development, staging, and production configurations
2. **Secure sensitive data**: Use secrets management systems for database credentials
3. **Set appropriate timeouts**: Configure connection and query timeouts based on your workload
4. **Enable structured logging**: Use JSON logging for production environments

## Docker Deployment

### Basic Dockerfile

```dockerfile
# Build stage
FROM rust:1.87 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/my-eventcore-app /usr/local/bin/
EXPOSE 8080
CMD ["my-eventcore-app"]
```

### Optimized Multi-Stage Build

```dockerfile
# Dependencies stage
FROM rust:1.87 AS dependencies
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Build stage
FROM dependencies AS builder
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/my-eventcore-app /
EXPOSE 8080
ENTRYPOINT ["/my-eventcore-app"]
```

### Docker Compose Example

```yaml
version: '3.8'

services:
  app:
    build: .
    ports:
      - "8080:8080"
    environment:
      DATABASE_URL: postgres://eventcore:password@postgres:5432/eventcore
      RUST_LOG: info,eventcore=debug
    depends_on:
      postgres:
        condition: service_healthy
    restart: unless-stopped

  postgres:
    image: postgres:17
    environment:
      POSTGRES_DB: eventcore
      POSTGRES_USER: eventcore
      POSTGRES_PASSWORD: password
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U eventcore"]
      interval: 5s
      timeout: 5s
      retries: 5

volumes:
  postgres_data:
```

## Kubernetes Deployment

### Basic Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: eventcore-app
  labels:
    app: eventcore-app
spec:
  replicas: 3
  selector:
    matchLabels:
      app: eventcore-app
  template:
    metadata:
      labels:
        app: eventcore-app
    spec:
      containers:
      - name: app
        image: myregistry/eventcore-app:v1.0.0
        ports:
        - containerPort: 8080
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: eventcore-secrets
              key: database-url
        - name: RUST_LOG
          value: "info,eventcore=debug"
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
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
```

### Service Configuration

```yaml
apiVersion: v1
kind: Service
metadata:
  name: eventcore-app
spec:
  selector:
    app: eventcore-app
  ports:
    - protocol: TCP
      port: 80
      targetPort: 8080
  type: LoadBalancer
```

### ConfigMap for Application Config

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: eventcore-config
data:
  app.yaml: |
    server:
      port: 8080
      workers: 4
    
    database:
      pool_size: 20
      connection_timeout: 30s
      max_lifetime: 1800s
    
    monitoring:
      metrics_port: 9090
      health_check_interval: 30s
```

### Secrets Management

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: eventcore-secrets
type: Opaque
stringData:
  database-url: postgres://user:password@postgres-service:5432/eventcore
```

### Horizontal Pod Autoscaler

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: eventcore-app-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: eventcore-app
  minReplicas: 3
  maxReplicas: 20
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
```

## Bare Metal Deployment

### systemd Service

```ini
[Unit]
Description=EventCore Application
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=eventcore
Group=eventcore
WorkingDirectory=/opt/eventcore
ExecStart=/opt/eventcore/bin/eventcore-app
Restart=always
RestartSec=10

# Environment
Environment="DATABASE_URL=postgres://eventcore:password@localhost:5432/eventcore"
Environment="RUST_LOG=info,eventcore=debug"
Environment="APP_PORT=8080"

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/log/eventcore

# Resource limits
LimitNOFILE=65536
MemoryLimit=1G
CPUQuota=200%

[Install]
WantedBy=multi-user.target
```

### Nginx Reverse Proxy

```nginx
upstream eventcore_backend {
    server 127.0.0.1:8080 max_fails=3 fail_timeout=30s;
    server 127.0.0.1:8081 max_fails=3 fail_timeout=30s;
    server 127.0.0.1:8082 max_fails=3 fail_timeout=30s;
    
    keepalive 32;
}

server {
    listen 80;
    server_name api.example.com;

    location / {
        proxy_pass http://eventcore_backend;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        
        # Connection pooling
        proxy_set_header Connection "";
        
        # Timeouts
        proxy_connect_timeout 5s;
        proxy_send_timeout 30s;
        proxy_read_timeout 30s;
    }

    location /health {
        proxy_pass http://eventcore_backend/health;
        access_log off;
    }
}
```

## Database Deployment Considerations

### PostgreSQL Configuration

```sql
-- Recommended PostgreSQL settings for EventCore
ALTER SYSTEM SET max_connections = 200;
ALTER SYSTEM SET shared_buffers = '256MB';
ALTER SYSTEM SET effective_cache_size = '1GB';
ALTER SYSTEM SET work_mem = '4MB';
ALTER SYSTEM SET maintenance_work_mem = '64MB';
ALTER SYSTEM SET random_page_cost = 1.1;
ALTER SYSTEM SET effective_io_concurrency = 200;
ALTER SYSTEM SET wal_level = replica;
ALTER SYSTEM SET max_wal_size = '1GB';
ALTER SYSTEM SET min_wal_size = '80MB';
```

### Database Migrations

```bash
#!/bin/bash
# Run database migrations before starting the application

# Wait for database to be ready
until PGPASSWORD=$DB_PASSWORD psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -c '\q'; do
  >&2 echo "Postgres is unavailable - sleeping"
  sleep 1
done

# Run migrations
eventcore-migrate up --database-url "$DATABASE_URL"

# Start the application
exec "$@"
```

## Production Checklist

### Pre-Deployment

- [ ] **Build optimization**: Use `--release` flag and enable LTO
- [ ] **Security audit**: Run `cargo audit` to check for vulnerabilities
- [ ] **Resource limits**: Set appropriate memory and CPU limits
- [ ] **Health checks**: Implement health and readiness endpoints
- [ ] **Monitoring**: Configure metrics and tracing exporters
- [ ] **Secrets management**: Use proper secrets storage (not environment variables in plain text)
- [ ] **Database setup**: Create indexes and configure connection pooling

### Deployment

- [ ] **Rolling updates**: Use zero-downtime deployment strategies
- [ ] **Database migrations**: Run migrations before deploying new code
- [ ] **Load balancing**: Configure proper load balancing with health checks
- [ ] **Connection draining**: Implement graceful shutdown
- [ ] **Monitoring alerts**: Set up alerts for key metrics
- [ ] **Backup strategy**: Implement database backup and recovery procedures

### Post-Deployment

- [ ] **Smoke tests**: Verify core functionality after deployment
- [ ] **Performance monitoring**: Watch for performance degradation
- [ ] **Error tracking**: Monitor error rates and types
- [ ] **Resource usage**: Track CPU, memory, and connection pool usage
- [ ] **Audit logging**: Ensure audit events are being recorded

## Scaling Considerations

### Horizontal Scaling

EventCore applications can scale horizontally with these considerations:

1. **Stateless design**: Ensure your application doesn't rely on local state
2. **Connection pooling**: Configure pools appropriately for your replica count
3. **Load balancing**: Use appropriate load balancing algorithms (round-robin, least-connections)
4. **Session affinity**: Not required for EventCore applications

### Vertical Scaling

When to scale vertically:

1. **Large event payloads**: More memory may be needed for processing
2. **Complex projections**: CPU-intensive projection rebuilds
3. **High concurrency**: More CPU cores for handling concurrent commands

### Database Scaling

1. **Read replicas**: Use for read-heavy projection queries
2. **Connection pooling**: Use PgBouncer or similar for connection multiplexing
3. **Partitioning**: Consider partitioning events table by date/stream
4. **Archive strategy**: Move old events to cold storage

## Troubleshooting Deployment Issues

### Common Issues

1. **Connection pool exhaustion**
   ```bash
   # Check active connections
   SELECT count(*) FROM pg_stat_activity WHERE state = 'active';
   ```

2. **Memory leaks**
   ```bash
   # Monitor memory usage
   docker stats <container_id>
   ```

3. **Slow startup**
   - Check database connectivity
   - Verify migration status
   - Review initialization logs

### Debug Commands

```bash
# Check application logs
kubectl logs -f deployment/eventcore-app

# Database connectivity test
kubectl exec -it deployment/eventcore-app -- psql $DATABASE_URL -c "SELECT 1"

# Resource usage
kubectl top pods -l app=eventcore-app

# Event debugging
kubectl describe pod <pod-name>
```

## Security Best Practices

1. **Run as non-root user**: Use a dedicated service account
2. **Network policies**: Restrict database access to application pods only
3. **TLS everywhere**: Encrypt all network traffic
4. **Secrets rotation**: Implement automatic credential rotation
5. **Audit logging**: Enable comprehensive audit trails
6. **Resource quotas**: Prevent resource exhaustion attacks

## Performance Tuning

### Application Level

```rust
// Configure connection pool for production
let pool_config = PoolConfig::default()
    .max_connections(100)
    .min_connections(10)
    .connection_timeout(Duration::from_secs(30))
    .idle_timeout(Some(Duration::from_secs(600)))
    .max_lifetime(Some(Duration::from_secs(1800)));
```

### Database Level

```sql
-- Create appropriate indexes
CREATE INDEX idx_events_stream_id_version ON events(stream_id, version);
CREATE INDEX idx_events_created_at ON events(created_at);
CREATE INDEX idx_events_event_type ON events(event_type);

-- Analyze tables regularly
ANALYZE events;
```

### Network Level

- Use connection pooling
- Enable HTTP/2 for gRPC traffic
- Configure TCP keepalive settings
- Use CDN for static assets

## Monitoring Integration

See the [Monitoring and Observability Guide](monitoring-and-observability.md) for detailed information on:

- OpenTelemetry configuration
- Prometheus metrics export
- Distributed tracing setup
- Custom dashboards and alerts

## Next Steps

- Review the [Operations Guide](operations-guide.md) for day-to-day operational procedures
- Check the [Troubleshooting Guide](troubleshooting.md) for common issues
- See [Performance Monitoring](performance-monitoring.md) for optimization strategies