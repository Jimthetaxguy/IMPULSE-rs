---
title: Claude Historian MCP -- Pattern Extraction Reference
status: active
phase: 1
audience: builder
tags: [jsonl, tfidf, scoring, parsing, search, mcp, reference-pattern]
---

# Claude Historian MCP -- Pattern Extraction Reference

> **Source:** `cloned-repos/claude-historian-mcp/src/`
> **Purpose:** Distill reusable patterns for Impulse's SessionEnd extraction, GENOME ranking, and search infrastructure.
> **Last extracted:** 2026-02-21

---

## 1. JSONL Streaming Parser

**Pattern:** Line-by-line streaming via Node `readline` over `createReadStream`. Each line is independently parsed -- malformed lines are skipped, never blocking the pipeline.

**Key properties:**

- Backpressure-safe via `for await...of` on the readline interface
- `crlfDelay: Infinity` handles Windows line endings
- Per-line try-catch: a single corrupt JSON line does not abort the file
- Outer try-catch: a missing/unreadable file returns an empty array (graceful degradation)
- Optional `timeFilter` callback applied before content extraction (skip early)
- Optional `query` string drives relevance scoring at parse time

```typescript
import { createReadStream } from 'fs';
import { createInterface } from 'readline';

async function parseJsonlFile(
  filePath: string,
  query?: string,
  timeFilter?: (timestamp: string) => boolean
): Promise<CompactMessage[]> {
  const messages: CompactMessage[] = [];

  try {
    const fileStream = createReadStream(filePath, { encoding: 'utf8' });
    const rl = createInterface({
      input: fileStream,
      crlfDelay: Infinity,
    });

    for await (const line of rl) {
      if (!line.trim()) continue;

      try {
        const raw = JSON.parse(line);

        // Early exit: skip lines outside time window
        if (timeFilter && !timeFilter(raw.timestamp)) continue;

        const content = extractContent(raw.message || {});
        if (!content) continue;

        messages.push({
          uuid: raw.uuid,
          timestamp: raw.timestamp,
          type: raw.type,
          content: smartTruncate(content, getContentLimit(content)),
          sessionId: raw.sessionId,
          relevanceScore: query ? calculateRelevanceScore(raw, query) : 0,
          context: extractContext(raw, content),
        });
      } catch {
        // Skip malformed line -- never block
        continue;
      }
    }
  } catch {
    // File unreadable -- return empty (graceful degradation)
  }

  return messages;
}
```

**Impulse application:** The SessionEnd hook reads Claude Code's JSONL transcript. This exact streaming pattern -- line-by-line, skip-on-error, early-filter -- is the right approach for Impulse's transcript reader.

---

## 2. Type Definitions

### ClaudeMessage (raw JSONL line)

The on-disk format written by Claude Code. Each line of a `.jsonl` session file is one of these.

```typescript
interface ClaudeMessage {
  parentUuid: string | null;
  isSidechain: boolean;
  userType: string;
  cwd: string;
  sessionId: string;
  version: string;
  type: 'user' | 'assistant' | 'tool_use' | 'tool_result';
  message?: {
    role: string;
    content: string | any[]; // String for simple, array for tool_use blocks
    id?: string;
    model?: string;
    usage?: {
      input_tokens: number;
      output_tokens: number;
      cache_creation_input_tokens?: number;
      cache_read_input_tokens?: number;
    };
  };
  uuid: string;
  timestamp: string;
  requestId?: string;
}
```

### CompactMessage (processed/scored)

The historian's intermediate representation. Content is truncated, context is extracted, relevance is scored.

```typescript
interface CompactMessage {
  uuid: string;
  timestamp: string;
  type: 'user' | 'assistant' | 'tool_use' | 'tool_result';
  content: string; // Truncated via smart preservation
  sessionId: string;
  projectPath?: string;
  relevanceScore?: number; // From TF-IDF scoring
  finalScore?: number; // After semantic boosts and coverage ratio
  context?: {
    filesReferenced?: string[];
    toolsUsed?: string[];
    errorPatterns?: string[];
    bashCommands?: string[];
    claudeInsights?: string[]; // Solutions, explanations from Claude
    codeSnippets?: string[];
    actionItems?: string[];
  };
}
```

### SearchResult

Returned from the main `searchConversations()` entry point.

```typescript
interface SearchResult {
  messages: CompactMessage[];
  totalResults: number;
  searchQuery: string;
  executionTime: number;
}
```

### FileContext

Tracks a file's history across conversations.

```typescript
interface FileContext {
  filePath: string;
  lastModified: string;
  relatedMessages: CompactMessage[];
  operationType: 'read' | 'write' | 'edit' | 'delete';
  changeFrequency?: number;
  impactLevel?: 'low' | 'medium' | 'high';
  affectedSystems?: string[];
}
```

**Impulse application:** Impulse's types should be simpler (we produce GENOME entries, not search results), but the `context` sub-object pattern -- extracting files, tools, errors, insights from raw messages -- maps directly to what SessionEnd extraction needs to produce.

---

## 3. TF-IDF Scoring Algorithm

The historian uses a multi-stage scoring pipeline rather than classic TF-IDF. The algorithm:

1. **Core term matching** -- Tech names from `CORE_TECH_PATTERN` regex MUST match. If the query contains "react" but the content does not, the message is **rejected entirely** (returns -1000).
2. **Supporting term scoring** -- Non-core, non-generic words of 5+ characters get `SUPPORTING_TERM_SCORE` (3 points) each.
3. **Word matching** -- General word matches get `WORD_MATCH_SCORE` (2 points).
4. **Phrase and majority bonuses** -- Exact phrase match and 60%+ word coverage get flat bonuses.
5. **Context scoring** -- Tool usage, file references, project match add flat scores.
6. **Post-scoring boosts** -- Coverage ratio, semantic boosts, recency decay applied multiplicatively.

### Core scoring function

```typescript
function calculateRelevanceScore(message: any, query: string, projectPath?: string): number {
  const coreScore = scoreCoreTerms(message, query);

  // If core terms don't match, reject completely
  if (coreScore < 0) return 0;

  let score = coreScore;
  score += scoreSupportingTerms(message, query);
  score += scoreToolUsage(message);
  score += scoreFileReferences(message);
  score += scoreProjectMatch(message, projectPath);
  return score;
}
```

### Post-scoring: coverage ratio boosting

After initial scoring, the search engine applies coverage-ratio boosting:

```typescript
// Coverage ratio: penalize partial matches
const matchCount = queryTerms.filter((term) => contentLower.includes(term)).length;
const coverageRatio = queryTerms.length > 0 ? matchCount / queryTerms.length : 1;

if (coverageRatio >= 0.5) {
  // Good coverage: each matching term doubles relevance
  for (const term of queryTerms) {
    if (contentLower.includes(term)) {
      score *= 2.0;
    }
  }
} else if (matchCount > 0) {
  // Partial: modest boost with coverage penalty
  score *= (1 + matchCount * 0.5) * coverageRatio;
} else {
  // No matches: heavy penalty
  score *= 0.1;
}
```

### Multi-word query gate

A critical quality rule: for queries with 2+ words, the scorer requires at least 2 word matches. This prevents false positives where "react hooks" matches content that only mentions "hooks" in a non-React context.

```typescript
if (queryWordPairs.length >= 2 && matches.length < 2) {
  return 0; // Multi-word queries MUST match multiple words
}
```

**Impulse application:** For GENOME ranking (deciding what to keep), the importance scoring from `SearchHelpers.calculateImportanceScore` is more relevant than query-based TF-IDF. But the coverage ratio technique and the multi-word gate are directly applicable to Impulse's future search over HISTORY_INDEX.md.

---

## 4. Scoring Constants

These are the tuned weights used throughout the scoring pipeline:

```typescript
// Core scoring weights
const EXACT_MATCH_SCORE = 10; // Exact tech term match (e.g., "react" matches "React")
const SUPPORTING_TERM_SCORE = 3; // 5+ char supporting terms
const WORD_MATCH_SCORE = 2; // General word matches
const EXACT_PHRASE_BONUS = 5; // Full query phrase appears in content
const MAJORITY_MATCH_BONUS = 4; // 60%+ of query words match

// Context scoring weights
const TOOL_USAGE_SCORE = 5; // Message uses tools
const FILE_REFERENCE_SCORE = 3; // Contains file paths
const PROJECT_MATCH_SCORE = 5; // Matches project context
```

### CORE_TECH_PATTERN regex

This regex identifies "must-match" technology terms. If the query contains one and the content does not, the entire message is rejected.

```typescript
const CORE_TECH_PATTERN =
  /^(webpack|docker|react|vue|angular|node|npm|yarn|typescript|python|
    rust|go|java|kubernetes|aws|gcp|azure|postgres|mysql|redis|mongodb|
    graphql|rest|grpc|oauth|jwt|git|github|gitlab|jenkins|nginx|apache|
    eslint|prettier|babel|vite|rollup|esbuild|jest|mocha|cypress|
    playwright|nextjs|nuxt|svelte|tailwind|sass|less|vitest|pnpm|
    turborepo|prisma|drizzle|sequelize|sqlite|leveldb|indexeddb)$/i;
```

### GENERIC_TERMS set

A blocklist of ~210 common words (action words, process terms, generic programming terms) that should NOT be treated as core terms even if they are 5+ characters. Examples: `config`, `setup`, `install`, `build`, `deploy`, `test`, `function`, `class`, `component`, `database`, `schema`, `server`, `client`, `interface`, `async`, `promise`.

**Design insight:** The interplay between CORE_TECH_PATTERN (must-match) and GENERIC_TERMS (never-core) creates a three-tier classification:

1. **Core tech** -- MUST be present; rejection if absent
2. **Supporting** -- 5+ chars, not generic, not core; scored as `SUPPORTING_TERM_SCORE`
3. **Generic** -- Ignored for core matching; still counted as `WORD_MATCH_SCORE`

**Impulse application:** For Impulse's GENOME deduplication, a similar three-tier approach could classify facts: **core decisions** (always keep), **supporting context** (keep if space), **generic observations** (discard on conflict).

---

## 5. Query Classification

Queries are classified into four types, which drive downstream behavior (intent matching, semantic boosts, content filtering):

```typescript
function classifyQueryType(query: string): 'error' | 'implementation' | 'analysis' | 'general' {
  const lowerQuery = query.toLowerCase();

  if (
    lowerQuery.includes('error') ||
    lowerQuery.includes('bug') ||
    lowerQuery.includes('fix') ||
    lowerQuery.includes('issue')
  ) {
    return 'error';
  }
  if (
    lowerQuery.includes('implement') ||
    lowerQuery.includes('create') ||
    lowerQuery.includes('build') ||
    lowerQuery.includes('add')
  ) {
    return 'implementation';
  }
  if (
    lowerQuery.includes('how') ||
    lowerQuery.includes('why') ||
    lowerQuery.includes('analyze') ||
    lowerQuery.includes('understand')
  ) {
    return 'analysis';
  }
  return 'general';
}
```

### Intent-based result filtering

Each query type applies a different relevance filter to candidate messages:

| Type             | Requires in content                                                  |
| ---------------- | -------------------------------------------------------------------- |
| `error`          | "error", "fix", "solution", or `errorPatterns` in context            |
| `implementation` | "implement", "create", "function", or `codeSnippets` in context      |
| `analysis`       | "analyze", "understand", "explain", or assistant message > 100 chars |
| `general`        | Tool usage in context, or assistant message > 80 chars               |

### Semantic boosts (multiplicative)

Query-derived boosts are applied multiplicatively after base scoring:

```typescript
function getSemanticBoosts(query: string): Record<string, number> {
  const boosts: Record<string, number> = {};
  if (query.includes('error')) boosts.errorResolution = 3.0;
  if (query.includes('fix')) boosts.solutions = 2.8;
  if (query.includes('implement')) boosts.implementation = 2.5;
  if (query.includes('tool')) boosts.toolUsage = 2.2;
  if (query.includes('file')) boosts.fileOperations = 2.0;
  if (query.includes('optimize')) boosts.optimization = 2.0;
  return boosts;
}
```

**Impulse application:** Impulse does not do user-facing search in Phase 1, but the classification pattern is useful for SessionEnd extraction -- classifying conversation _segments_ (debugging session, implementation session, planning session) to decide what GENOME entries to extract.

---

## 6. Parallel Search

The historian processes multiple projects and files concurrently using `Promise.allSettled`. The pattern has three key properties:

1. **Fault isolation** -- `allSettled` (not `all`) ensures one failed project/file does not abort the search.
2. **Early termination** -- After aggregating results from parallel batches, the loop breaks if enough high-quality candidates are found.
3. **Bounded concurrency** -- Project count is capped (`maxProjects = min(dirs.length, max(8, ceil(limit/2)))`) and files-per-project is capped at 4.

```typescript
async function gatherRelevantCandidates(
  projectDirs: string[],
  query: string,
  targetCount: number
): Promise<CompactMessage[]> {
  const candidates: CompactMessage[] = [];

  // Process projects in parallel with fault isolation
  const projectResults = await Promise.allSettled(
    projectDirs.map(async (projectDir) => {
      const dirCandidates = await processProjectFocused(
        projectDir,
        query,
        Math.ceil(targetCount / projectDirs.length)
      );
      return dirCandidates;
    })
  );

  // Aggregate with quality filtering
  for (const result of projectResults) {
    if (result.status === 'fulfilled') {
      const dirMessages = result.value.filter((msg) => isHighlyRelevant(msg, query));
      candidates.push(...dirMessages);

      // Early termination: enough high-quality candidates
      if (candidates.length >= targetCount) break;
    }
  }

  return candidates;
}
```

### Nested parallelism

Within each project, files are also processed in parallel:

```typescript
const fileResults = await Promise.allSettled(
  jsonlFiles
    .slice(0, Math.min(jsonlFiles.length, 8)) // Cap files per project
    .map((file) => processJsonlFile(projectDir, file, query, timeFilter))
);
```

### Cache with LRU-like eviction

Parsed file results are cached in a `Map<string, CompactMessage[]>` (up to 500 entries). When full, the least-valuable cache entry (lowest average relevance score) is evicted to make room for high-value new content:

```typescript
if (messageCache.size >= 500 && messages.some((m) => (m.relevanceScore || 0) > 8)) {
  const leastValuable = findLeastValuableCacheEntry();
  messageCache.delete(leastValuable.key);
  messageCache.set(cacheKey, messages);
}
```

**Impulse application:** Impulse's SessionEnd processes a single transcript file, so the multi-project parallel pattern is not directly needed. However, the `Promise.allSettled` + early-termination pattern is useful if Impulse ever needs to scan multiple `.impulse/` directories (e.g., a global search across projects).

---

## 7. Content Truncation

The historian uses a multi-strategy content preservation system. Content type is detected first, then the appropriate truncation strategy is applied.

### Content type detection

````typescript
function detectContentType(content: string): 'code' | 'error' | 'technical' | 'conversational' {
  if (
    content.includes('```') ||
    content.includes('function ') ||
    content.includes('const ') ||
    content.includes('import ')
  ) {
    return 'code';
  }
  if (content.match(/(error|exception|failed)/i) && content.match(/at \w+|line \d+|:\d+:\d+/)) {
    return 'error';
  }
  if (content.match(/\.(ts|js|json|md|py)\b/) || content.includes('src/')) {
    return 'technical';
  }
  return 'conversational';
}
````

### Adaptive content limits

Content type determines the truncation budget:

| Content type     | Max length |
| ---------------- | ---------- |
| `code`           | 4000 chars |
| `error`          | 3500 chars |
| `technical`      | 3500 chars |
| `conversational` | 3000 chars |

### Code block preservation

For code content, the algorithm tries to keep complete code blocks (triple-backtick fenced). If a block cannot fit entirely, it includes preceding context and truncates the block:

````typescript
function preserveCodeBlocks(content: string, maxLength: number): string {
  const codeBlockRegex = /```[\s\S]*?```/g;
  const codeBlocks = content.match(codeBlockRegex) || [];

  if (codeBlocks.length > 0) {
    let preserved = '';
    let remainingLength = maxLength;

    for (const block of codeBlocks) {
      if (block.length <= remainingLength) {
        preserved += block + '\n';
        remainingLength -= block.length + 1;
      } else {
        // Partial: include context before block + truncated block
        const contextBefore = content.substring(0, content.indexOf(block)).slice(-100);
        preserved +=
          contextBefore + block.substring(0, remainingLength - contextBefore.length - 3) + '...';
        break;
      }
    }
    return preserved.trim();
  }
  return preserveTechnicalContent(content, maxLength);
}
````

### Intelligent truncation (fallback)

When no special structure is detected, truncation tries natural boundaries in priority order:

```typescript
function intelligentTruncation(content: string, maxLength: number): string {
  if (content.length <= maxLength) return content;

  const boundaries = ['\n\n', '. ', '! ', '? ', '\n', ', ', ' '];

  for (const boundary of boundaries) {
    const lastBoundary = content.lastIndexOf(boundary, maxLength - 3);
    if (lastBoundary > maxLength * 0.7) {
      // Don't truncate too early
      return content.substring(0, lastBoundary) + '...';
    }
  }

  return content.substring(0, maxLength - 3) + '...';
}
```

### Sentence-value scoring

For conversational content, sentences are scored by information density and the highest-value sentences are selected:

- **High-value terms** (`solution`, `fix`, `error`, `function`, `const`, `import`, etc.): +2 each
- **Code/technical references** (backticks, `/`, `.ts`, `.js`): +3
- **Outcome language** (`now`, `result`, `this will`): +2
- **Short sentences** (< 40 chars): -1
- **Noise patterns** (`this session is being continued`, `system-reminder`): -50

**Impulse application:** Impulse's SessionEnd extraction needs to preserve decisions and solutions while discarding noise. The sentence-value scoring approach is directly applicable -- score transcript segments by information density, keep only high-value ones for GENOME.md.

---

## 8. Deduplication

The historian uses three levels of deduplication:

### Level 1: Content signature deduplication

Creates a normalized signature from content + context, keeps the higher-scored duplicate.

```typescript
static createContentSignature(message: CompactMessage): string {
  const content = message.content.toLowerCase();
  const files = (message.context?.filesReferenced || []).sort().join('|');
  const tools = (message.context?.toolsUsed || []).sort().join('|');
  const errors = (message.context?.errorPatterns || []).join('|');

  const normalizedContent = content
    .replace(/\d+/g, 'N')       // Replace numbers with placeholder
    .replace(/['"]/g, '')        // Remove quotes
    .replace(/\s+/g, ' ')       // Normalize whitespace
    .substring(0, 200);          // First 200 chars only

  return `${files}:${tools}:${errors}:${normalizedContent}`;
}
```

### Level 2: Intelligent signature (search results)

A richer signature used during search result deduplication:

```typescript
function createIntelligentSignature(message: CompactMessage): string {
  const contentHash = message.content
    .toLowerCase()
    .replace(/\d+/g, 'N')
    .replace(/["']/g, '')
    .replace(/\s+/g, ' ')
    .substring(0, 80);

  const tools = (message.context?.toolsUsed || []).sort().join('|');
  const files = (message.context?.filesReferenced || []).length > 0 ? 'files' : 'nofiles';

  return `${message.type}:${tools}:${files}:${contentHash}`;
}
```

### Level 3: Jaccard similarity (formatter)

For final display deduplication, a word-level Jaccard similarity check eliminates messages that are > 80% similar:

```typescript
function calculateSimilarity(text1: string, text2: string): number {
  const words1 = new Set(text1.toLowerCase().split(/\s+/));
  const words2 = new Set(text2.toLowerCase().split(/\s+/));
  const intersection = new Set([...words1].filter((x) => words2.has(x)));
  const union = new Set([...words1, ...words2]);
  return intersection.size / union.size; // Jaccard index
}

// Used as: if (calculateSimilarity(msg.content, existing.content) > 0.8) skip
```

### Fuzzy word matching

For query similarity (finding "similar past questions"), character-level comparison with a 60% threshold:

```typescript
static isWordSimilar(word1: string, word2: string): boolean {
  if (Math.abs(word1.length - word2.length) > 3) return false;
  const minLen = Math.min(word1.length, word2.length);
  if (minLen < 4) return false;

  let matches = 0;
  for (let i = 0; i < minLen; i++) {
    if (word1[i] === word2[i]) matches++;
  }
  return matches >= minLen * 0.6;
}
```

**Impulse application:** GENOME.md deduplication is a core requirement (the spec mandates "deduplication at write time"). The content-signature approach -- normalize numbers, strip quotes, collapse whitespace, then compare a prefix -- is the right starting point. For Impulse, the signature should also factor in the GENOME section (decisions vs. preferences vs. patterns).

---

## 9. Importance Scoring ("Pain to Rediscover")

The `SearchHelpers.calculateImportanceScore` function scores content by how painful it would be to rediscover. This is distinct from query-relevance scoring -- it measures intrinsic value.

```typescript
static calculateImportanceScore(content: string): number {
  let maxBoost = 1.0;

  // Decisions (highest value: 2.5x)
  const decisionPatterns = [
    'decided to', 'decision', 'chose', 'trade-off', 'rationale',
    'why we', 'instead of', 'opted for', 'architecture', 'design decision'
  ];
  if (decisionPatterns.some(p => content.includes(p))) {
    maxBoost = Math.max(maxBoost, 2.5);
  }

  // Bugfixes (high value: 2.0x)
  const bugfixPatterns = [
    'fixed', 'bug', 'gotcha', 'workaround', 'edge case',
    'issue', 'problem', 'broke', 'breaking'
  ];
  if (bugfixPatterns.some(p => content.includes(p))) {
    maxBoost = Math.max(maxBoost, 2.0);
  }

  // Features (moderate value: 1.5x)
  const featurePatterns = [
    'implemented', 'shipped', 'feature', 'added', 'built',
    'created', 'new', 'release'
  ];
  if (featurePatterns.some(p => content.includes(p))) {
    maxBoost = Math.max(maxBoost, 1.5);
  }

  // Discoveries (learning value: 1.3x)
  const discoveryPatterns = [
    'learned', 'discovered', 'insight', 'found out',
    'realize', 'understanding', 'now know'
  ];
  if (discoveryPatterns.some(p => content.includes(p))) {
    maxBoost = Math.max(maxBoost, 1.3);
  }

  return maxBoost;
}
```

**Hierarchy:** Decisions (2.5x) > Bugfixes (2.0x) > Features (1.5x) > Discoveries (1.3x)

**Impulse application:** This is the most directly applicable pattern for Impulse. GENOME.md should prioritize exactly this hierarchy -- architectural decisions and resolved debates at the top, followed by gotchas/workarounds, then shipped features, then learnings. The SessionEnd LLM prompt should explicitly ask for these categories.

---

## 10. Application to Impulse

| Historian Pattern                       | Impulse Component                    | How It Maps                                                                                               |
| --------------------------------------- | ------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| JSONL streaming parser                  | SessionEnd hook transcript reader    | Same pattern: `createReadStream` + `readline`, line-by-line, skip malformed, graceful degradation         |
| `CompactMessage.context`                | GENOME.md fact entries               | Extract files, tools, errors, decisions from transcript segments                                          |
| Content type detection                  | SessionEnd extraction prioritization | Classify transcript segments to weight what gets extracted                                                |
| Sentence-value scoring                  | GENOME entry ranking                 | Score extracted facts by information density; drop low-value observations                                 |
| Importance scoring (pain-to-rediscover) | GENOME section assignment            | Decisions (2.5x) go to top of GENOME; gotchas next; features and learnings last                           |
| Content signature dedup                 | GENOME.md deduplication              | Normalize + prefix-hash to detect when a new fact duplicates an existing GENOME entry                     |
| Intelligent truncation                  | GENOME entry formatting              | Preserve code blocks and error messages fully; truncate conversational filler at sentence boundaries      |
| Query classification                    | Future: HISTORY_INDEX search         | Classify search intent to filter history index entries                                                    |
| Parallel search + early termination     | Future: cross-project GENOME search  | `Promise.allSettled` across `.impulse/` directories with early exit                                       |
| CORE_TECH_PATTERN / GENERIC_TERMS       | GENOME fact classification           | Three-tier: core decisions (must-keep), supporting context (keep-if-space), generic (discard-on-conflict) |
| Noise filtering patterns                | SessionEnd transcript cleaning       | Same noise strings apply: "this session is being continued", system reminders, short acknowledgments      |

### Priority patterns for Phase 1 implementation

1. **JSONL streaming parser** -- Needed immediately for SessionEnd transcript reading
2. **Importance scoring** -- Needed for GENOME entry ranking during extraction
3. **Content signature dedup** -- Needed to prevent GENOME bloat across sessions
4. **Noise filtering** -- Needed to skip low-value transcript lines before LLM extraction
5. **Intelligent truncation** -- Needed to keep GENOME entries concise

### Patterns deferred to Phase 2+

- Query classification and TF-IDF scoring (no user-facing search in Phase 1)
- Parallel search across projects (single-project scope in Phase 1)
- Semantic boosts and coverage ratio (requires search infrastructure)
- Cache with LRU eviction (not needed for single-session processing)

---

_Extracted from `cloned-repos/claude-historian-mcp/src/` -- files: parser.ts, types.ts, scoring-constants.ts, utils.ts, search.ts, search-helpers.ts, formatter.ts_
