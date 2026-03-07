/**
 * Test helpers and custom assertions
 */

import { expect } from 'vitest';
import type { Pattern, Vector, HarnessEvent } from '../types.js';

// ============================================================================
// Custom Assertions
// ============================================================================

/**
 * Assert that a pattern contains valid SWARM provenance header
 */
export function assertValidSWARMProvenance(pattern: Pattern) {
  const match = pattern.suggestedInjection.match(/^\[SWARM:(.+?):(\d+\.\d+)\]/);
  expect(match).toBeDefined();
  if (match) {
    expect(match[1]).toMatch(/^agent-/); // Agent ID
    const confidence = parseFloat(match[2]);
    expect(confidence).toBeGreaterThanOrEqual(0);
    expect(confidence).toBeLessThanOrEqual(1);
  }
}

/**
 * Assert that a pattern is NOT a SWARM injection (for echo testing)
 */
export function assertNotSWARMInjection(pattern: Pattern) {
  expect(pattern.suggestedInjection).not.toMatch(/^\[SWARM:/);
}

/**
 * Assert vector has correct dimensions
 */
export function assertValidVector(vector: Vector, expectedDim = 384) {
  expect(vector.vector).toBeInstanceOf(Float32Array);
  expect(vector.vector.length).toBe(expectedDim);

  // Check that vector has non-zero values (not degenerate)
  const sumSquares = Array.from(vector.vector).reduce(
    (sum, v) => sum + v * v,
    0,
  );
  expect(sumSquares).toBeGreaterThan(0.01);
}

/**
 * Assert event is valid according to schema
 */
export function assertValidEvent(event: HarnessEvent) {
  expect(event.type).toMatch(/^(message\.updated|tool\.execute)$/);
  expect(event.timestamp).toBeGreaterThan(0);
  expect(event.agentId).toBeDefined();
  expect(event.agentId.length).toBeGreaterThan(0);
}

/**
 * Assert confidence decay formula is applied correctly
 */
export function assertConfidenceDecay(
  initialConfidence: number,
  decayedConfidence: number,
  minutesElapsed: number,
  lambda = 0.03,
) {
  const expected = initialConfidence * Math.exp(-lambda * minutesElapsed);
  expect(decayedConfidence).toBeCloseTo(expected, 5);
}

/**
 * Assert two vectors have expected cosine similarity
 */
export function assertCosineSimilarity(
  v1: Float32Array,
  v2: Float32Array,
  expectedSimilarity: number,
  tolerance = 0.01,
) {
  const dotProduct = Array.from(v1).reduce(
    (sum, a, i) => sum + a * v2[i],
    0,
  );
  const norm1 = Math.sqrt(
    Array.from(v1).reduce((sum, a) => sum + a * a, 0),
  );
  const norm2 = Math.sqrt(
    Array.from(v2).reduce((sum, a) => sum + a * a, 0),
  );
  const similarity = dotProduct / (norm1 * norm2);

  expect(similarity).toBeCloseTo(expectedSimilarity, 2);
}

// ============================================================================
// Test Utilities
// ============================================================================

/**
 * Wait for a condition to be true (with timeout)
 */
export async function waitFor(
  condition: () => boolean | Promise<boolean>,
  timeoutMs = 5000,
  checkIntervalMs = 100,
): Promise<void> {
  const startTime = Date.now();
  while (Date.now() - startTime < timeoutMs) {
    if (await condition()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, checkIntervalMs));
  }
  throw new Error(`Timeout waiting for condition after ${timeoutMs}ms`);
}

/**
 * Create a temporary test database path
 */
export function createTempDbPath(): string {
  const crypto = require('crypto');
  return `/tmp/test-swarm-${crypto.randomUUID()}.db`;
}

/**
 * Measure function execution time
 */
export async function measureTime<T>(
  fn: () => Promise<T>,
): Promise<{ result: T; durationMs: number }> {
  const start = Date.now();
  const result = await fn();
  const durationMs = Date.now() - start;
  return { result, durationMs };
}

// ============================================================================
// Mock/Stub Utilities
// ============================================================================

/**
 * Create a mock event stream
 */
export async function* mockEventStream(
  events: any[],
  intervalMs = 0,
) {
  for (const event of events) {
    yield event;
    if (intervalMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  }
}
