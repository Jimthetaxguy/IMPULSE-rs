/**
 * Core type definitions for SWARM harness
 *
 * Schema: See docs/DATA-MODELS.md for full specification
 */

import { z } from 'zod';

// ============================================================================
// Event Types
// ============================================================================

export const MessageEventSchema = z.object({
  type: z.literal('message.updated'),
  timestamp: z.number(),
  agentId: z.string(),
  role: z.enum(['user', 'assistant', 'system']),
  content: z.string(),
  metadata: z.record(z.unknown()).optional(),
});
export type MessageEvent = z.infer<typeof MessageEventSchema>;

export const ToolExecuteEventSchema = z.object({
  type: z.literal('tool.execute'),
  timestamp: z.number(),
  agentId: z.string(),
  toolName: z.string(),
  toolArgs: z.record(z.unknown()),
  status: z.enum(['before', 'after', 'error']),
  result: z.unknown().optional(),
  duration: z.number().optional(),
});
export type ToolExecuteEvent = z.infer<typeof ToolExecuteEventSchema>;

export type HarnessEvent = MessageEvent | ToolExecuteEvent;

// ============================================================================
// Vector & Pattern Types
// ============================================================================

export const VectorSchema = z.object({
  id: z.string(),
  agentId: z.string(),
  partition: z.string(), // file path or topic
  vector: z.instanceof(Float32Array),
  confidence: z.number().min(0).max(1),
  sourceEvents: z.array(z.string()),
  createdAt: z.number(),
  decayedAt: z.number().optional(),
});
export type Vector = z.infer<typeof VectorSchema>;

export const PatternSchema = z.object({
  id: z.string(),
  sourceAgents: z.array(z.string()),
  similarity: z.number().min(0).max(1),
  extractedTopic: z.string(),
  suggestedInjection: z.string().max(120), // Token limit
  confidenceScore: z.number().min(0).max(1),
  detectedAt: z.number(),
  fileScope: z.array(z.string()).optional(),
});
export type Pattern = z.infer<typeof PatternSchema>;

// ============================================================================
// Database Schema
// ============================================================================

export const StoredEventSchema = z.object({
  id: z.string(),
  type: z.string(),
  agentId: z.string(),
  data: z.string(), // JSON-serialized
  createdAt: z.number(),
  expiresAt: z.number(),
});
export type StoredEvent = z.infer<typeof StoredEventSchema>;

// ============================================================================
// Configuration
// ============================================================================

export const HarnessConfigSchema = z.object({
  databasePath: z.string().default('./live_state.db'),
  liveMarkdownPath: z.string().default('./LIVE.md'),
  logLevel: z.enum(['debug', 'info', 'warn', 'error']).default('info'),
  vectorDimension: z.number().default(384),
  similarityThreshold: z.number().min(0).max(1).default(0.88),
  confidenceDecayLambda: z.number().default(0.03),
  tokenBudgetThresholds: z.object({
    normal: z.number().default(0.7),
    compressed: z.number().default(0.9),
  }),
  rateLimit: z.object({
    injectionPerAgentMs: z.number().default(45000),
  }),
});
export type HarnessConfig = z.infer<typeof HarnessConfigSchema>;

// ============================================================================
// Error Types
// ============================================================================

export class HarnessError extends Error {
  constructor(
    message: string,
    public code: string,
    public context?: Record<string, unknown>,
  ) {
    super(message);
    this.name = 'HarnessError';
  }
}

export const ErrorCodes = {
  DB_INIT_FAILED: 'DB_INIT_FAILED',
  DB_WRITE_FAILED: 'DB_WRITE_FAILED',
  DB_READ_FAILED: 'DB_READ_FAILED',
  EMBEDDING_FAILED: 'EMBEDDING_FAILED',
  PATTERN_DETECTION_FAILED: 'PATTERN_DETECTION_FAILED',
  INJECTION_FAILED: 'INJECTION_FAILED',
  HOOK_SUBSCRIPTION_FAILED: 'HOOK_SUBSCRIPTION_FAILED',
  LIVE_MD_WRITE_FAILED: 'LIVE_MD_WRITE_FAILED',
  CONFIG_INVALID: 'CONFIG_INVALID',
} as const;

// ============================================================================
// Metrics
// ============================================================================

export const MetricsSchema = z.object({
  eventsProcessed: z.number().default(0),
  patternsDetected: z.number().default(0),
  injectionsAttempted: z.number().default(0),
  injectionsFailed: z.number().default(0),
  echoesDetected: z.number().default(0),
  avgLatencyMs: z.number().default(0),
  memoryUsageMB: z.number().default(0),
});
export type Metrics = z.infer<typeof MetricsSchema>;
