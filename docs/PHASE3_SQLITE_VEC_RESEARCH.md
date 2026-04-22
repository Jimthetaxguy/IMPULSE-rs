# Phase 3: SQLite-Vec Research for Semantic Search

## Overview

`sqlite-vec` is a vector search SQLite extension that enables semantic search capabilities within SQLite. It's the successor to `sqlite-vss` and is designed to run anywhere SQLite runs.

## Key Features

- **Multi-format vectors**: Supports float, int8, and binary vectors
- **Zero dependencies**: Written in pure C
- **Cross-platform**: Works on Linux, MacOS, Windows, WASM (browser), Raspberry Pi
- **Virtual tables**: Uses `vec0` virtual table for vector storage
- **KNN queries**: Built-in nearest-neighbor search with distance metrics

## Installation

```bash
# Python
pip install sqlite-vec

# Node.js
npm install sqlite-vec

# Rust
cargo add sqlite-vec

# Go
go get -u github.com/asg017/sqlite-vec/bindings/go
```

## Usage Example

```sql
.load ./vec0

-- Create vector table
CREATE VIRTUAL TABLE embeddings USING vec0(
  id INTEGER PRIMARY KEY,
  text TEXT,
  embedding float[384]
);

-- Insert vectors
INSERT INTO embeddings (id, text, embedding)
VALUES (1, 'session tracked main.rs', '[-0.2, 0.25, ...]');

-- Semantic search
SELECT id, text, distance
FROM embeddings
WHERE embedding match '[0.89, 0.54, ...]'
ORDER BY distance
LIMIT 5;
```

## Integration with Impulse

### For Semantic Search (Phase 3)

The Phase 3 roadmap mentions "Semantic search" using sqlite-vec. This would enable:

1. **Session similarity**: Find similar past sessions based on files/tools
2. **Context retrieval**: Semantic matching of relevant history for chat
3. **Pattern discovery**: Find recurring patterns across sessions

### Implementation Path

1. **Add dependency**:

   ```toml
   # Cargo.toml
   sqlite-vec = "0.1"
   ```

2. **Create embedding module**:
   - Generate embeddings using local models (e.g., via `sqlite-rembed` or `sqlite-lembed`)
   - Store in vec0 tables with metadata (session_id, timestamp, file paths)

3. **Query integration**:
   - Use for chat context retrieval
   - Session deduplication
   - "Dj Vu Trigger": proactive semantic matching

### Alternative: Python Integration

Since the AGENTS.md mentions `+Python` for semantic search, we could:

- Use Python's `sqlite-vec` package
- Generate embeddings via sentence-transformers
- Store in shared SQLite database

## Trade-offs

| Approach                       | Pros                           | Cons                                   |
| ------------------------------ | ------------------------------ | -------------------------------------- |
| sqlite-vec (Rust)              | Zero deps, fast, single binary | Need to generate embeddings separately |
| Python + sentence-transformers | Rich embedding models          | Additional runtime dependency          |
| Remote API (OpenAI)            | Powerful embeddings            | Requires API key, network              |

## Recommendation

For Impulse's Phase 3:

1. Start with **sqlite-vec** for storage/query
2. Use local embeddings (via `sqlite-lembed` or Ollama) for privacy
3. Fall back to Python pipeline for complex embedding generation

## References

- [sqlite-vec GitHub](https://github.com/asg017/sqlite-vec)
- [Documentation](https://alexgarcia.xyz/sqlite-vec/)
- [sqlite-rembed](https://github.com/asg017/sqlite-rembed) - Remote API embeddings
- [sqlite-lembed](https://github.com/asg017/sqlite-lembed) - Local GGUF embeddings
