/**
 * LIVE.md writer: Maintains real-time state document
 *
 * Template:
 * - Active agents and current messages
 * - Detected patterns
 * - System metrics
 * - Session timeline
 *
 * Invariant: LIVE.md is disposable view-layer (never source of truth)
 */

import { writeFileSync } from 'fs';
import { logger } from '../utils/logger.js';
import type { DatabaseConnection } from '../types.js';

export class LiveMarkdownWriter {
  private lastRefresh = 0;
  private refreshInterval = 2000; // Update every 2s

  constructor(
    private filePath: string,
    private db: DatabaseConnection,
  ) {}

  /**
   * Initialize LIVE.md
   */
  async initialize(): Promise<void> {
    try {
      this.writeFile(this.getInitialTemplate());
      logger.info('LIVE.md initialized', { path: this.filePath });
    } catch (error) {
      logger.error('Failed to initialize LIVE.md', { error });
      throw error;
    }
  }

  /**
   * Refresh LIVE.md (debounced)
   */
  async refresh(): Promise<void> {
    const now = Date.now();
    if (now - this.lastRefresh < this.refreshInterval) {
      return; // Debounced
    }

    try {
      const content = await this.generateContent();
      this.writeFile(content);
      this.lastRefresh = now;
    } catch (error) {
      logger.error('Failed to refresh LIVE.md', { error });
    }
  }

  /**
   * Generate LIVE.md content
   */
  private async generateContent(): Promise<string> {
    // TODO: Query DB for:
    // - Recent events
    // - Active agents
    // - Detected patterns
    // - Metrics
    return this.getPlaceholderTemplate();
  }

  /**
   * Write content to file
   */
  private writeFile(content: string): void {
    try {
      writeFileSync(this.filePath, content, 'utf-8');
    } catch (error) {
      logger.error('Failed to write LIVE.md', { error, path: this.filePath });
      throw error;
    }
  }

  /**
   * Initial template (session start)
   */
  private getInitialTemplate(): string {
    return `# SWARM Session Live State

**Session Start:** ${new Date().toISOString()}
**Status:** Initializing...

## Active Agents
(none yet)

## Detected Patterns
(none yet)

## Metrics
- Events processed: 0
- Patterns detected: 0
- Injections sent: 0
- Echoes detected: 0

## Timeline
(events will appear here)
`;
  }

  /**
   * Placeholder template (while DB queries are implemented)
   */
  private getPlaceholderTemplate(): string {
    return `# SWARM Session Live State

**Session Start:** ${new Date().toISOString()}
**Last Update:** ${new Date().toISOString()}
**Status:** Running

## Active Agents
(implement: query agents from events table)

## Detected Patterns
(implement: query patterns from patterns table)

## Metrics
- Events processed: (TBD)
- Patterns detected: (TBD)
- Injections sent: (TBD)
- Echoes detected: (TBD)

## Timeline
(implement: recent event stream)
`;
  }
}
