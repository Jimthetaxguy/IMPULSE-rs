/**
 * Database layer: sqlite-vec integration
 *
 * Schema:
 * - events: All incoming events (message.updated, tool.execute)
 * - vectors: Embeddings from events (384-dim)
 * - patterns: Detected patterns (triggers for injection)
 * - metadata: System state (timestamps, flags)
 *
 * Invariant: All writes are idempotent
 */

import Database from 'better-sqlite3';
import { logger } from '../utils/logger.js';
import type { HarnessEvent, StoredEvent } from '../types.js';

export class DatabaseConnection {
  private db: Database.Database;
  private initialized = false;

  constructor(filePath: string) {
    this.db = new Database(filePath);
    this.db.pragma('journal_mode = WAL');
    this.db.pragma('synchronous = NORMAL');
    logger.debug('Database connection opened', { path: filePath });
  }

  /**
   * Initialize schema
   */
  async initialize(): Promise<void> {
    if (this.initialized) return;

    try {
      // Create events table
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS events (
          id TEXT PRIMARY KEY,
          type TEXT NOT NULL,
          agent_id TEXT NOT NULL,
          data TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_agent ON events(agent_id);
        CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at);
      `);

      // Create vectors table (sqlite-vec virtual table)
      this.db.exec(`
        CREATE VIRTUAL TABLE IF NOT EXISTS vectors USING vec0(
          id TEXT PRIMARY KEY,
          agent_id TEXT,
          partition TEXT,
          embedding FLOAT32[384],
          confidence FLOAT,
          source_events TEXT,
          created_at INTEGER
        );
      `);

      // Create patterns table
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS patterns (
          id TEXT PRIMARY KEY,
          source_agents TEXT NOT NULL,
          similarity FLOAT NOT NULL,
          extracted_topic TEXT NOT NULL,
          suggested_injection TEXT NOT NULL,
          confidence_score FLOAT NOT NULL,
          detected_at INTEGER NOT NULL,
          file_scope TEXT,
          created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_patterns_created ON patterns(created_at);
      `);

      // Create metadata table
      this.db.exec(`
        CREATE TABLE IF NOT EXISTS metadata (
          key TEXT PRIMARY KEY,
          value TEXT,
          updated_at INTEGER
        );
      `);

      this.initialized = true;
      logger.info('Database schema initialized');
    } catch (error) {
      logger.error('Failed to initialize database schema', { error });
      throw error;
    }
  }

  /**
   * Store event
   */
  async storeEvent(event: HarnessEvent): Promise<void> {
    try {
      const expiresAt = Date.now() + 24 * 60 * 60 * 1000; // 24h retention
      const stmt = this.db.prepare(`
        INSERT OR REPLACE INTO events (id, type, agent_id, data, created_at, expires_at)
        VALUES (?, ?, ?, ?, ?, ?)
      `);

      stmt.run(
        `${event.type}-${event.agentId}-${event.timestamp}`,
        event.type,
        event.agentId,
        JSON.stringify(event),
        event.timestamp,
        expiresAt,
      );

      logger.debug('Event stored', {
        type: event.type,
        agentId: event.agentId,
      });
    } catch (error) {
      logger.error('Failed to store event', { error, event });
      throw error;
    }
  }

  /**
   * Get recent events (for pattern detection context)
   */
  getRecentEvents(
    agentId: string,
    limitHours = 1,
    limit = 8,
  ): HarnessEvent[] {
    try {
      const cutoff = Date.now() - limitHours * 60 * 60 * 1000;
      const stmt = this.db.prepare(`
        SELECT data FROM events
        WHERE agent_id = ? AND created_at > ?
        ORDER BY created_at DESC
        LIMIT ?
      `);

      const rows = stmt.all(agentId, cutoff, limit) as Array<{
        data: string;
      }>;
      return rows.map((row) => JSON.parse(row.data) as HarnessEvent);
    } catch (error) {
      logger.error('Failed to get recent events', { error, agentId });
      throw error;
    }
  }

  /**
   * Clean up expired events (TTL)
   */
  async cleanupExpiredEvents(): Promise<number> {
    try {
      const stmt = this.db.prepare(
        'DELETE FROM events WHERE expires_at < ?',
      );
      const result = stmt.run(Date.now());
      logger.debug('Cleaned up expired events', { count: result.changes });
      return result.changes;
    } catch (error) {
      logger.error('Failed to cleanup expired events', { error });
      throw error;
    }
  }

  /**
   * Close database connection
   */
  async close(): Promise<void> {
    try {
      this.db.close();
      logger.debug('Database connection closed');
    } catch (error) {
      logger.error('Failed to close database', { error });
      throw error;
    }
  }
}

// Export as singleton pattern or factory
export const Database = DatabaseConnection;
