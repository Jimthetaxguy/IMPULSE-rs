/**
 * Test fixtures and factories for SWARM harness tests
 *
 * Pattern: Use factory functions to create test objects with sane defaults
 * that can be overridden per-test.
 */

import { v4 as uuid } from 'uuid';
import type {
  MessageEvent,
  ToolExecuteEvent,
  HarnessEvent,
  Vector,
  Pattern,
} from '../types.js';

// ============================================================================
// Event Factories
// ============================================================================

export function createMessageEvent(
  overrides?: Partial<MessageEvent>,
): MessageEvent {
  return {
    type: 'message.updated',
    timestamp: Date.now(),
    agentId: overrides?.agentId || `agent-${uuid().slice(0, 8)}`,
    role: overrides?.role || 'assistant',
    content: overrides?.content || 'Test message',
    metadata: overrides?.metadata,
    ...overrides,
  };
}

export function createToolExecuteEvent(
  overrides?: Partial<ToolExecuteEvent>,
): ToolExecuteEvent {
  return {
    type: 'tool.execute',
    timestamp: Date.now(),
    agentId: overrides?.agentId || `agent-${uuid().slice(0, 8)}`,
    toolName: overrides?.toolName || 'test-tool',
    toolArgs: overrides?.toolArgs || {},
    status: overrides?.status || 'after',
    result: overrides?.result,
    duration: overrides?.duration || 100,
    ...overrides,
  };
}

export function createHarnessEvent(
  type: 'message' | 'tool' = 'message',
  overrides?: Partial<HarnessEvent>,
): HarnessEvent {
  return type === 'message'
    ? createMessageEvent(overrides as Partial<MessageEvent>)
    : createToolExecuteEvent(overrides as Partial<ToolExecuteEvent>);
}

// ============================================================================
// Vector Factories
// ============================================================================

export function createVector(overrides?: Partial<Vector>): Vector {
  const dim = 384;
  const vector = new Float32Array(dim);
  for (let i = 0; i < dim; i++) {
    vector[i] = Math.random();
  }

  return {
    id: overrides?.id || uuid(),
    agentId: overrides?.agentId || `agent-${uuid().slice(0, 8)}`,
    partition: overrides?.partition || 'default',
    vector,
    confidence: overrides?.confidence || 0.95,
    sourceEvents: overrides?.sourceEvents || [uuid()],
    createdAt: overrides?.createdAt || Date.now(),
    decayedAt: overrides?.decayedAt,
    ...overrides,
  };
}

// ============================================================================
// Pattern Factories
// ============================================================================

export function createPattern(overrides?: Partial<Pattern>): Pattern {
  return {
    id: overrides?.id || uuid(),
    sourceAgents: overrides?.sourceAgents || [
      `agent-${uuid().slice(0, 8)}`,
      `agent-${uuid().slice(0, 8)}`,
    ],
    similarity: overrides?.similarity || 0.92,
    extractedTopic: overrides?.extractedTopic || 'shared-topic',
    suggestedInjection:
      overrides?.suggestedInjection ||
      '[SWARM:agent-1:0.92] Both agents working on auth module',
    confidenceScore: overrides?.confidenceScore || 0.92,
    detectedAt: overrides?.detectedAt || Date.now(),
    fileScope: overrides?.fileScope || ['src/auth.ts'],
    ...overrides,
  };
}

// ============================================================================
// Batch Factories
// ============================================================================

export function createMultipleEvents(
  count: number,
  agentIds?: string[],
): MessageEvent[] {
  const agents = agentIds || [
    'agent-1',
    'agent-2',
    'agent-3',
    'agent-4',
    'agent-5',
  ];

  return Array.from({ length: count }, (_, i) =>
    createMessageEvent({
      agentId: agents[i % agents.length],
      content: `Message ${i + 1}`,
      timestamp: Date.now() + i * 100,
    }),
  );
}

export function createMultiplePatterns(count: number): Pattern[] {
  return Array.from({ length: count }, () => createPattern());
}
