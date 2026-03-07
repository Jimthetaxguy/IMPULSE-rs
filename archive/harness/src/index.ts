#!/usr/bin/env bun
/**
 * SWARM Harness Entry Point
 *
 * Subscribes to OpenCode plugin hooks and coordinates multi-agent workspace.
 * See: docs/ARCHITECTURE.md, docs/STEWARD.md
 */

import { Harness } from './harness.js';
import { logger } from './utils/logger.js';

async function main() {
  try {
    const harness = new Harness();
    await harness.start();
    logger.info('SWARM harness started successfully');
  } catch (error) {
    logger.error('Fatal error starting harness:', error);
    process.exit(1);
  }
}

main();
