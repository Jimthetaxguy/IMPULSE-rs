/**
 * Main Harness class
 *
 * Coordinates:
 * - OpenCode plugin hook subscriptions
 * - Event processing and storage
 * - Pattern detection
 * - LIVE.md state maintenance
 *
 * Invariants:
 * - All writes are idempotent (same event twice = same DB state)
 * - Anti-echo: never re-score patterns containing [SWARM] prefix
 * - Rate limit: max 1 injection per agent per 45s
 */

import { Database } from './db/database.js';
import { HookSubscriber } from './hooks/subscriber.js';
import { PatternDetector } from './pattern/detector.js';
import { LiveMarkdownWriter } from './live-md/writer.js';
import { logger } from './utils/logger.js';
import { MetricsCollector } from './metrics/collector.js';
import type { HarnessConfig, HarnessEvent } from './types.js';
import { HarnessConfigSchema } from './types.js';

export class Harness {
  private config: HarnessConfig;
  private db!: Database;
  private hooks!: HookSubscriber;
  private patternDetector!: PatternDetector;
  private liveWriter!: LiveMarkdownWriter;
  private metrics: MetricsCollector;

  constructor(configOverrides?: Partial<HarnessConfig>) {
    this.config = HarnessConfigSchema.parse(configOverrides || {});
    this.metrics = new MetricsCollector();
  }

  /**
   * Initialize and start the harness
   */
  async start(): Promise<void> {
    try {
      // 1. Initialize database
      this.db = new Database(this.config.databasePath);
      await this.db.initialize();
      logger.info('Database initialized', { path: this.config.databasePath });

      // 2. Initialize pattern detector
      this.patternDetector = new PatternDetector(
        this.db,
        this.config.vectorDimension,
        this.config.similarityThreshold,
      );
      logger.info('Pattern detector initialized');

      // 3. Initialize LIVE.md writer
      this.liveWriter = new LiveMarkdownWriter(
        this.config.liveMarkdownPath,
        this.db,
      );
      await this.liveWriter.initialize();
      logger.info('LIVE.md writer initialized', {
        path: this.config.liveMarkdownPath,
      });

      // 4. Subscribe to OpenCode hooks
      this.hooks = new HookSubscriber(this.config);
      this.hooks.on('event', (event: HarnessEvent) =>
        this.handleEvent(event),
      );
      await this.hooks.connect();
      logger.info('OpenCode hooks subscribed');
    } catch (error) {
      logger.error('Failed to start harness', { error });
      throw error;
    }
  }

  /**
   * Handle incoming event from OpenCode hook
   */
  private async handleEvent(event: HarnessEvent): Promise<void> {
    const startTime = Date.now();

    try {
      // 1. Store event
      await this.db.storeEvent(event);
      this.metrics.recordEvent();

      // 2. Detect patterns (only for message events)
      if (event.type === 'message.updated') {
        const patterns = await this.patternDetector.detect(event);
        if (patterns.length > 0) {
          this.metrics.recordPatterns(patterns.length);
          logger.debug('Patterns detected', { count: patterns.length });
        }
      }

      // 3. Update LIVE.md
      await this.liveWriter.refresh();

      // 4. Record metrics
      const duration = Date.now() - startTime;
      this.metrics.recordLatency(duration);
      logger.debug('Event processed', { agentId: event.agentId, duration });
    } catch (error) {
      logger.error('Error processing event', { event, error });
      this.metrics.recordError();
    }
  }

  /**
   * Shutdown the harness gracefully
   */
  async shutdown(): Promise<void> {
    logger.info('Shutting down harness');
    await this.hooks?.disconnect();
    await this.db?.close();
    logger.info('Harness shutdown complete');
  }

  /**
   * Get current metrics
   */
  getMetrics() {
    return this.metrics.snapshot();
  }
}
