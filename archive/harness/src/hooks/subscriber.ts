/**
 * OpenCode plugin hook subscriber
 *
 * Subscribes to:
 * - message.updated: New messages in chat
 * - tool.execute.before/after: Tool invocations
 * - experimental.session.compacting: Context compaction points (injection site)
 */

import { EventEmitter } from 'events';
import { logger } from '../utils/logger.js';
import type { HarnessEvent, HarnessConfig } from '../types.js';

export class HookSubscriber extends EventEmitter {
  private connected = false;
  private subscriptions = new Set<string>();

  constructor(private config: HarnessConfig) {
    super();
  }

  /**
   * Connect to OpenCode plugin API
   */
  async connect(): Promise<void> {
    try {
      // TODO: Implement OpenCode plugin SDK connection
      // See: cloned-repos/opencode/packages/plugin/src/index.ts
      // Pattern: REST API calls to subscribe to hooks

      this.subscriptions.add('message.updated');
      this.subscriptions.add('tool.execute.after');
      this.subscriptions.add('experimental.session.compacting');

      this.connected = true;
      logger.info('OpenCode hooks subscribed', {
        subscriptions: Array.from(this.subscriptions),
      });
    } catch (error) {
      logger.error('Failed to connect to OpenCode', { error });
      throw error;
    }
  }

  /**
   * Disconnect from OpenCode
   */
  async disconnect(): Promise<void> {
    this.connected = false;
    this.subscriptions.clear();
    logger.info('OpenCode hooks disconnected');
  }

  /**
   * Handle incoming event from OpenCode
   */
  private handleOpenCodeEvent(rawEvent: any): void {
    try {
      const event = this.parseEvent(rawEvent);
      this.emit('event', event);
    } catch (error) {
      logger.error('Failed to parse OpenCode event', { error, rawEvent });
    }
  }

  /**
   * Parse raw OpenCode event into HarnessEvent
   */
  private parseEvent(rawEvent: any): HarnessEvent {
    // TODO: Implement parsing logic based on OpenCode SDK
    throw new Error('Not implemented');
  }
}
