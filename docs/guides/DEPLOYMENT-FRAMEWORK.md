---
status: superseded
phase: 1
audience: builder
tags: [guide, deployment, npm]
last_updated: 2026-02-20
---

# Deployment Framework: From Dev to Production

> **Historical SWARM/TypeScript deployment design — superseded.** It is not a deployment contract
> for the current local Rust product. Current executable and external-system boundaries are
> documented in [`../../README.md`](../../README.md) and the canonical Rust contract.

> **Version:** 1.0 | **Status:** Design | **Updated:** 2026-02-20
> **Scope:** Docker, Kubernetes, CI/CD (GitHub Actions), monitoring, SLOs

---

## Overview

This framework defines how SWARM goes from local development → staging → production. It covers:

1. **Containerization** (Docker multi-stage builds, minimal images)
2. **Orchestration** (Kubernetes deployment manifests)
3. **CI/CD** (GitHub Actions automated testing, linting, deployment)
4. **Observability** (Structured logging, metrics, traces)
5. **Reliability** (SLOs, error budgets, incident response)

---

## Core Principle: Immutable Infrastructure

Once built and tested, an image never changes. Configuration lives in:
- Environment variables (secrets)
- ConfigMaps (non-secrets)
- Helm values (Kubernetes)

This prevents "it works on my machine" and ensures reproducibility.

---

## Part 1: Containerization

### Multi-Stage Docker Build

```dockerfile
# Dockerfile for SWARM harness

# Stage 1: Builder (TypeScript compilation)
FROM oven/bun:1-alpine AS builder
WORKDIR /build

COPY harness/package.json harness/package-lock.json ./
RUN bun install

COPY harness/src ./src
COPY harness/tsconfig.json ./
RUN bun run build          # Outputs to dist/

# Stage 2: Runtime (minimal image)
FROM oven/bun:1-alpine
RUN apk add --no-cache \
    sqlite \
    libgomp

WORKDIR /app

# Copy compiled code from builder
COPY --from=builder /build/dist ./dist
COPY --from=builder /build/package.json ./

# Copy vector DB (static)
COPY harness/data/live_state.db ./data/

# Entry point
ENTRYPOINT ["bun", "dist/index.js"]

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD bun run healthcheck || exit 1
```

**Image Size Targets:**
- Base (Bun + SQLite): ~50MB
- With dependencies: ~70MB
- With optional mem0: ~85MB

**Build Time:**
- Full build: ~60s
- Layer cache hit: ~5s (rebuild only changed layers)

### Zellij Plugin WASM Image

```dockerfile
# Dockerfile for Zellij plugin

FROM rust:1.75-alpine AS builder
RUN rustup target add wasm32-wasip1
WORKDIR /build

COPY zellij-plugins/memory-status-bar ./
RUN cargo build --release --target wasm32-wasip1

# Output: target/wasm32-wasip1/release/memory_status_bar.wasm

FROM scratch
COPY --from=builder /build/target/wasm32-wasip1/release/*.wasm ./
```

**Note:** WASM plugins don't need runtime—just copy the `.wasm` binary to Zellij plugins directory.

---

## Part 2: Kubernetes Deployment

### Helm Chart Structure

```
impulse-helm/
├── Chart.yaml                        # Chart metadata
├── values.yaml                       # Default config
├── values-prod.yaml                  # Production overrides
└── templates/
    ├── deployment.yaml               # SWARM harness deployment
    ├── service.yaml                  # Service (ClusterIP)
    ├── configmap.yaml                # Non-secrets
    ├── secret.yaml                   # Secrets (from external store)
    ├── ingress.yaml                  # Optional: HTTP access
    ├── hpa.yaml                      # Horizontal Pod Autoscaling
    └── pdb.yaml                      # Pod Disruption Budget
```

### Deployment Manifest (values-prod.yaml)

```yaml
# Kubernetes deployment for SWARM harness
replicaCount: 3

image:
  repository: docker.io/vibecodeprime/impulse
  tag: "1.0.0"
  pullPolicy: IfNotPresent

resources:
  requests:
    cpu: 100m
    memory: 256Mi
  limits:
    cpu: 500m
    memory: 512Mi

env:
  - name: SWARM_LOG_LEVEL
    value: "info"
  - name: SWARM_RATE_LIMIT_WINDOW_MS
    value: "45000"
  - name: SWARM_VECTOR_DIM
    value: "384"
  - name: SWARM_PATTERN_CONFIDENCE_THRESHOLD
    value: "0.88"

# Secrets (populated from external secret store)
secretEnv:
  - name: OPENCODE_API_KEY
    valueFrom:
      secretKeyRef:
        name: impulse-secrets
        key: opencode-api-key

# Health checks
livenessProbe:
  httpGet:
    path: /health
    port: 8080
  initialDelaySeconds: 10
  periodSeconds: 30
  timeoutSeconds: 3
  failureThreshold: 3

readinessProbe:
  httpGet:
    path: /ready
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 10
  timeoutSeconds: 1
  failureThreshold: 2

# Autoscaling
autoscaling:
  enabled: true
  minReplicas: 3
  maxReplicas: 10
  targetCPUUtilizationPercentage: 70
  targetMemoryUtilizationPercentage: 80

# Pod Disruption Budget (survive node drains)
podDisruptionBudget:
  enabled: true
  minAvailable: 1

# Network policies
networkPolicy:
  enabled: true
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              name: impulse
  egress:
    - to:
        - podSelector:
            matchLabels:
              app: sqlite-vec
      ports:
        - protocol: TCP
          port: 5432
```

---

## Part 3: CI/CD Pipeline (GitHub Actions)

### Workflow: Test, Build, Deploy

```yaml
# .github/workflows/deploy.yml

name: Test, Build, Deploy

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

env:
  REGISTRY: docker.io
  IMAGE_NAME: vibecodeprime/impulse

jobs:
  # Stage 1: Lint & Type Check (fail fast)
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Setup Bun
        uses: oven-sh/setup-bun@v1

      - name: Install dependencies
        working-directory: harness
        run: bun install

      - name: Lint
        working-directory: harness
        run: bun run lint

      - name: TypeScript check
        working-directory: harness
        run: bun run tsc --noEmit

  # Stage 2: Unit Tests
  unit-tests:
    runs-on: ubuntu-latest
    needs: lint
    steps:
      - uses: actions/checkout@v3
      - uses: oven-sh/setup-bun@v1

      - name: Install
        working-directory: harness
        run: bun install

      - name: Run unit tests
        working-directory: harness
        run: bun run test

      - name: Upload coverage
        uses: codecov/codecov-action@v3
        with:
          files: ./harness/coverage/coverage-final.json

  # Stage 3: Integration Tests
  integration-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    services:
      sqlite:
        image: sqlite:latest
        options: >-
          --health-cmd "sqlite3 /tmp/test.db '.quit'"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
    steps:
      - uses: actions/checkout@v3
      - uses: oven-sh/setup-bun@v1

      - name: Run integration tests
        working-directory: harness
        run: bun run test:integration --reporter=verbose

      - name: Run 6-agent stress test
        working-directory: harness
        run: bun run test:stress --agents=6 --events=100

  # Stage 4: Build Docker image
  build:
    runs-on: ubuntu-latest
    needs: [unit-tests, integration-tests]
    if: github.ref == 'refs/heads/main' || github.ref == 'refs/heads/develop'
    outputs:
      image-tag: ${{ steps.meta.outputs.tags }}
    steps:
      - uses: actions/checkout@v3

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v2

      - name: Log in to Docker Hub
        uses: docker/login-action@v2
        with:
          username: ${{ secrets.DOCKER_USERNAME }}
          password: ${{ secrets.DOCKER_PASSWORD }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v4
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=ref,event=branch
            type=semver,pattern={{version}}
            type=sha,prefix={{branch}}-

      - name: Build and push Docker image
        uses: docker/build-push-action@v4
        with:
          context: .
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=registry,ref=${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}:buildcache
          cache-to: type=registry,ref=${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}:buildcache,mode=max

  # Stage 5: Deploy to staging
  deploy-staging:
    runs-on: ubuntu-latest
    needs: build
    if: github.ref == 'refs/heads/develop'
    steps:
      - uses: actions/checkout@v3

      - name: Deploy to staging cluster
        run: |
          helm upgrade --install impulse ./impulse-helm \
            --namespace staging \
            --values impulse-helm/values-staging.yaml \
            --set image.tag=${{ needs.build.outputs.image-tag }}

      - name: Wait for rollout
        run: |
          kubectl rollout status deployment/impulse -n staging --timeout=5m

      - name: Run smoke tests
        run: |
          ./scripts/smoke-tests-staging.sh

  # Stage 6: Deploy to production (manual approval)
  deploy-production:
    runs-on: ubuntu-latest
    needs: deploy-staging
    if: github.ref == 'refs/heads/main'
    environment:
      name: production
      url: https://impulse.internal.company.com
    steps:
      - uses: actions/checkout@v3

      - name: Deploy to production cluster
        run: |
          helm upgrade --install impulse ./impulse-helm \
            --namespace production \
            --values impulse-helm/values-prod.yaml \
            --set image.tag=${{ github.sha }}

      - name: Verify deployment
        run: |
          kubectl rollout status deployment/impulse -n production --timeout=10m
          ./scripts/verify-production.sh

      - name: Notify Slack
        uses: slackapi/slack-github-action@v1
        with:
          webhook-url: ${{ secrets.SLACK_WEBHOOK }}
          payload: |
            {
              "text": "✅ SWARM deployed to production",
              "blocks": [
                {
                  "type": "section",
                  "text": {
                    "type": "mrkdwn",
                    "text": "*Deployment Status*\nCluster: production\nVersion: ${{ github.sha }}"
                  }
                }
              ]
            }
```

### Gating Policy

| Stage | Fail → Block | Pass → Continue |
|-------|--------------|-----------------|
| Lint | ✅ Must pass | Code quality enforced |
| Unit tests | ✅ Must pass | 85%+ coverage required |
| Integration | ✅ Must pass | 6-agent scenario must have 0 echoes |
| Build | ✅ Must pass | Image size < 100MB |
| Deploy staging | ⚠️ Advisory | If fails, must approve override |
| Deploy prod | ✅ Manual approval | Only after staging validates |

---

## Part 4: Observability

### Structured Logging

```typescript
// Harness logging setup
import pino from 'pino';

const logger = pino({
  level: process.env.LOG_LEVEL || 'info',
  transport: {
    target: 'pino-pretty',
    options: {
      colorize: true,
      singleLine: false,
      translateTime: 'SYS:standard',
      ignore: 'pid,hostname',
    },
  },
});

// Contextual logging
logger.info(
  {
    agent_id: 'claude-code-1',
    event_type: 'message.updated',
    pattern_detected: true,
    pattern_id: 'auth-refactor',
    confidence: 0.92,
    injection_sent: true,
  },
  'Pattern detected and injection sent'
);

// Error logging (with stack)
logger.error(
  {
    error: err,
    event_id: 'msg-123',
    context: { agent: 'opencode', file: 'src/auth.ts' },
  },
  'Failed to process event'
);
```

### Metrics Export (OpenTelemetry)

```typescript
import { MeterProvider, PeriodicExportingMetricReader } from '@opentelemetry/sdk-metrics';
import { OTLPMetricExporter } from '@opentelemetry/exporter-metrics-otlp-http';

const metricExporter = new OTLPMetricExporter({
  url: process.env.OTEL_EXPORTER_OTLP_ENDPOINT,
});

const meterProvider = new MeterProvider({
  readers: [new PeriodicExportingMetricReader(metricExporter)],
});

const meter = meterProvider.getMeter('impulse-harness');

// Key metrics
const patternDetectionLatency = meter.createHistogram('swarm.pattern_detection_ms');
const injectionQueueSize = meter.createUpDownCounter('swarm.injection_queue_size');
const echoLoopsDetected = meter.createCounter('swarm.echo_loops_total');
const vectorSearchLatency = meter.createHistogram('swarm.vector_search_ms');

// Usage
const start = Date.now();
await detector.detectPatterns(event);
patternDetectionLatency.record(Date.now() - start);
```

### Dashboards (Prometheus + Grafana)

```yaml
# grafana-dashboard.json excerpt
{
  "panels": [
    {
      "title": "Pattern Detection Latency (p50, p99)",
      "targets": [
        {
          "expr": "histogram_quantile(0.50, swarm_pattern_detection_ms_bucket)"
        },
        {
          "expr": "histogram_quantile(0.99, swarm_pattern_detection_ms_bucket)"
        }
      ]
    },
    {
      "title": "Echo Loops Detected (per hour)",
      "targets": [
        {
          "expr": "rate(swarm_echo_loops_total[1h])"
        }
      ]
    },
    {
      "title": "Memory Usage (resident set)",
      "targets": [
        {
          "expr": "process_resident_memory_bytes"
        }
      ]
    },
    {
      "title": "Vector Search Latency",
      "targets": [
        {
          "expr": "histogram_quantile(0.95, swarm_vector_search_ms_bucket)"
        }
      ]
    }
  ]
}
```

---

## Part 5: SLOs & Error Budgets

### Service Level Objectives

| SLO | Target | Measurement |
|-----|--------|-------------|
| **Availability** | 99.5% (3.6h downtime/month) | HTTP 200 / total requests |
| **Pattern Detection Latency** | p95 < 500ms | histogram_quantile(0.95, swarm_pattern_detection_ms_bucket) |
| **Vector Search Latency** | p95 < 200ms | histogram_quantile(0.95, swarm_vector_search_ms_bucket) |
| **Injection Success Rate** | 99% | successful_injections / total_attempted |
| **Zero Echo Cascades** | 100% | echo_loops_detected == 0 |

### Error Budget Policy

```
Monthly Error Budget:
├─ Availability: 0.5% (4.32 hours downtime allowed)
├─ Latency: 5% of requests >500ms allowed
├─ Injection failures: 1% (1 failure per 100) allowed
└─ Echo loops: 0 (non-negotiable)

Burn Rate:
├─ Fast burn (2x daily budget): Page on-call
├─ Slow burn (1x daily budget): Create incident
├─ No burn: Business as usual

Quarterly Review:
├─ If budget remaining: Can take engineering risk (refactor, new features)
├─ If budget exhausted: Focus on stability (bugs, performance, observability)
```

---

## Part 6: Incident Response

### Playbooks

#### Playbook: High Latency (Pattern Detection >1s)

```
1. Alert fires: swarm_pattern_detection_p95 > 1000ms
2. Page on-call engineer
3. Triage:
   - Check vector DB query time (slow vector search?)
   - Check embedder latency (local model?, API latency?)
   - Check harness memory (GC thrashing?)
   - Check event volume (rate spike?)

4. Mitigation options:
   a. Increase cache size (reduce embedding calls)
   b. Reduce vector dimension (384→192, faster search)
   c. Scale up resources (more CPU for embedder)
   d. Rate limit event ingestion (backpressure)

5. Resolution:
   - Implement fix in canary deployment
   - Monitor for 15 minutes
   - Rollout to 25%, 50%, 100% if stable
```

#### Playbook: Echo Cascade Detected

```
1. Alert fires: swarm_echo_loops_total > 0
2. Immediate action: Increase confidence threshold (0.88 → 0.92)
3. Kill injection goroutine (stop new injections)
4. Investigate:
   - Which pattern triggered cascade?
   - Which agents were involved?
   - What was the similarity score?

5. Root cause analysis:
   - Did runaway detector fail?
   - Was anti-echo check bypassed?
   - Are we injecting stale [SWARM:] prefixed content?

6. Resolution:
   - Fix detected issue
   - Decrease threshold back to 0.88
   - Re-enable injection
   - Verify 0 cascades for 1 hour
```

---

## Part 7: Checklist: From Dev to Prod

- [ ] **Code Quality**
  - [ ] Linting passes
  - [ ] TypeScript strict mode
  - [ ] 85%+ test coverage
  - [ ] No console.log (use structured logging)

- [ ] **Performance**
  - [ ] Pattern detection <500ms (p95)
  - [ ] Vector search <200ms (p95)
  - [ ] Memory <512MB (limit)
  - [ ] CPU <500m (limit)

- [ ] **Reliability**
  - [ ] Health checks pass (liveness + readiness)
  - [ ] 6-agent scenario: 0 echo cascades
  - [ ] Stress test (1000+ events) passes
  - [ ] Graceful shutdown implemented

- [ ] **Observability**
  - [ ] Structured logging enabled
  - [ ] Key metrics exported (OTEL)
  - [ ] Dashboards visible in Grafana
  - [ ] Alerts configured (latency, errors, availability)

- [ ] **Deployment**
  - [ ] Docker image < 100MB
  - [ ] Helm values configured for staging + prod
  - [ ] Secrets populated (no hardcoding)
  - [ ] Network policies applied
  - [ ] Pod disruption budget set

- [ ] **Documentation**
  - [ ] Runbook for each SLO
  - [ ] Incident playbooks written
  - [ ] Deployment procedure documented
  - [ ] Rollback procedure tested

---

## References

- CI/CD: `docs/phases/PHASE1-CHECKLIST.md` § "Deployment"
- Testing: `docs/guides/TESTING-FRAMEWORK.md`
- Performance: `docs/guides/PERFORMANCE-PROFILING.md`

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Implementation_
