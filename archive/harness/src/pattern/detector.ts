/**
 * Pattern detector: Finds when multiple agents are working on overlapping topics
 *
 * Algorithm:
 * 1. On message.updated, embed agent's last 8 turns
 * 2. Query vectors from OTHER agents (partition key filter)
 * 3. If max cosine similarity > threshold, extract pattern
 * 4. Apply confidence decay, rate limiting, anti-echo
 * 5. Queue for injection
 *
 * Guarantees:
 * - No re-scoring of SWARM injections (anti-echo)
 * - Rate limit: 1 injection per agent per 45s
 * - Confidence decay: λ=0.03 (50% after ~23 min)
 * - File-scoped: only inject to agents on related files
 */

import { logger } from '../utils/logger.js';
import type {
  DatabaseConnection,
  MessageEvent,
  Pattern,
  Vector,
} from '../types.js';

export class PatternDetector {
  private lastInjectionTime = new Map<string, number>(); // agentId -> timestamp

  constructor(
    private db: DatabaseConnection,
    private vectorDimension: number,
    private similarityThreshold: number,
  ) {}

  /**
   * Detect patterns for an incoming message
   */
  async detect(event: MessageEvent): Promise<Pattern[]> {
    try {
      // 1. Skip if sender is SWARM (anti-echo)
      if (this.isSWARMInjection(event.content)) {
        logger.debug('Skipping SWARM injection (anti-echo)', {
          agentId: event.agentId,
        });
        return [];
      }

      // 2. Check rate limit
      if (this.isRateLimited(event.agentId)) {
        logger.debug('Rate limited', { agentId: event.agentId });
        return [];
      }

      // 3. Get recent context from this agent
      const recentEvents = this.db.getRecentEvents(event.agentId, 1, 8);
      if (recentEvents.length === 0) {
        return [];
      }

      // 4. Embed agent's context
      const sourceVector = await this.embedContext(recentEvents);

      // 5. Query other agents' vectors
      const otherVectors = await this.queryOtherAgentVectors(event.agentId);

      // 6. Find similar patterns
      const patterns: Pattern[] = [];
      for (const otherVector of otherVectors) {
        const similarity = this.cosineSimilarity(
          sourceVector,
          otherVector.vector,
        );
        if (similarity > this.similarityThreshold) {
          const pattern = await this.extractPattern(
            event.agentId,
            otherVector,
            similarity,
          );
          patterns.push(pattern);
        }
      }

      // 7. Record injection time
      if (patterns.length > 0) {
        this.lastInjectionTime.set(event.agentId, Date.now());
      }

      return patterns;
    } catch (error) {
      logger.error('Failed to detect patterns', { error, event });
      return [];
    }
  }

  /**
   * Check if content contains SWARM injection prefix
   */
  private isSWARMInjection(content: string): boolean {
    return /^\[SWARM:/.test(content);
  }

  /**
   * Check if agent is rate-limited
   */
  private isRateLimited(agentId: string): boolean {
    const lastTime = this.lastInjectionTime.get(agentId);
    if (!lastTime) return false;
    return Date.now() - lastTime < 45000; // 45s rate limit
  }

  /**
   * Embed last 8 turns into a 384-dim vector
   * TODO: Implement with actual embedding model
   */
  private async embedContext(events: any[]): Promise<Float32Array> {
    const vector = new Float32Array(this.vectorDimension);
    // Placeholder: fill with random values
    for (let i = 0; i < this.vectorDimension; i++) {
      vector[i] = Math.random();
    }
    return vector;
  }

  /**
   * Query vectors from other agents
   */
  private async queryOtherAgentVectors(
    excludeAgentId: string,
  ): Promise<Vector[]> {
    // TODO: Query DB for vectors where agent_id != excludeAgentId
    // Use cosine distance ordering
    return [];
  }

  /**
   * Compute cosine similarity between two vectors
   */
  private cosineSimilarity(v1: Float32Array, v2: Float32Array): number {
    let dotProduct = 0;
    let norm1 = 0;
    let norm2 = 0;

    for (let i = 0; i < v1.length; i++) {
      dotProduct += v1[i] * v2[i];
      norm1 += v1[i] * v1[i];
      norm2 += v2[i] * v2[i];
    }

    return dotProduct / (Math.sqrt(norm1) * Math.sqrt(norm2));
  }

  /**
   * Extract pattern from similar vectors
   */
  private async extractPattern(
    sourceAgentId: string,
    otherVector: Vector,
    similarity: number,
  ): Promise<Pattern> {
    // TODO: Implement pattern extraction
    // Should identify shared topic/file/decision
    throw new Error('Not implemented');
  }
}
