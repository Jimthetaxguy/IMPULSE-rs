/**
 * Metrics collector
 *
 * Tracks:
 * - Event processing rate and latency
 * - Pattern detection metrics
 * - Injection success/failure rates
 * - Echo detection counts
 * - Memory usage
 */

import type { Metrics } from '../types.js';

export class MetricsCollector {
  private metrics: Metrics = {
    eventsProcessed: 0,
    patternsDetected: 0,
    injectionsAttempted: 0,
    injectionsFailed: 0,
    echoesDetected: 0,
    avgLatencyMs: 0,
    memoryUsageMB: 0,
  };

  private latencies: number[] = [];
  private startTime = Date.now();

  /**
   * Record event processing
   */
  recordEvent(): void {
    this.metrics.eventsProcessed++;
  }

  /**
   * Record pattern detection
   */
  recordPatterns(count: number): void {
    this.metrics.patternsDetected += count;
  }

  /**
   * Record injection attempt
   */
  recordInjectionAttempt(success: boolean): void {
    this.metrics.injectionsAttempted++;
    if (!success) {
      this.metrics.injectionsFailed++;
    }
  }

  /**
   * Record echo detection
   */
  recordEcho(): void {
    this.metrics.echoesDetected++;
  }

  /**
   * Record latency measurement
   */
  recordLatency(durationMs: number): void {
    this.latencies.push(durationMs);

    // Keep only last 100 measurements
    if (this.latencies.length > 100) {
      this.latencies.shift();
    }

    // Calculate running average
    const sum = this.latencies.reduce((a, b) => a + b, 0);
    this.metrics.avgLatencyMs = sum / this.latencies.length;
  }

  /**
   * Record error (failed event processing)
   */
  recordError(): void {
    // Increment error count (could be added to metrics)
  }

  /**
   * Get current metrics snapshot
   */
  snapshot(): Metrics {
    const memUsage = process.memoryUsage();
    return {
      ...this.metrics,
      memoryUsageMB: Math.round(memUsage.heapUsed / 1024 / 1024 * 100) / 100,
    };
  }

  /**
   * Get uptime seconds
   */
  getUptimeSeconds(): number {
    return Math.floor((Date.now() - this.startTime) / 1000);
  }

  /**
   * Get events per second
   */
  getEventsPerSecond(): number {
    const uptime = this.getUptimeSeconds();
    return uptime > 0 ? this.metrics.eventsProcessed / uptime : 0;
  }

  /**
   * Reset metrics (for testing)
   */
  reset(): void {
    this.metrics = {
      eventsProcessed: 0,
      patternsDetected: 0,
      injectionsAttempted: 0,
      injectionsFailed: 0,
      echoesDetected: 0,
      avgLatencyMs: 0,
      memoryUsageMB: 0,
    };
    this.latencies = [];
    this.startTime = Date.now();
  }
}
