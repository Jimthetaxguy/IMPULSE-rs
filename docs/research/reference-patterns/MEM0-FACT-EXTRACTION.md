---
title: Mem0 Fact Extraction & Memory Architecture Patterns
description: Architectural patterns from mem0 for memory layer implementation
version: '1.0'
updated: 2026-02-21
type: reference
category: patterns
phase: phase2
status: active
audience: builders
tags: [mem0, fact-extraction, memory-hierarchy, vector-search, embeddings, reference-pattern]
source_repo: https://github.com/mem0ai/mem0
source_version: 'v1.0.0'
---

# Mem0 Fact Extraction & Memory Architecture Patterns

> **Purpose:** A distilled reference of the architectural patterns used in [mem0](https://github.com/mem0ai/mem0) -- the open-source memory layer for AI agents. This document captures the patterns, not the implementation details, so that Impulse Phase 2 can adopt or adapt them without needing the repo on disk.

---

## 1. Architecture Overview

Mem0 is structured as a **pluggable memory system** with four interchangeable layers connected through factory patterns and a unified configuration model.

```
                   MemoryConfig (Pydantic)
                        |
          +-------------+--------------+
          |             |              |
     LlmConfig    EmbedderConfig  VectorStoreConfig
          |             |              |
      LlmFactory   EmbedderFactory  VectorStoreFactory
          |             |              |
     LLMBase (ABC)  EmbeddingBase  VectorStoreBase
     /  |  \  ...    /  |  \        /  |  \  ...
  OpenAI Anthropic  OpenAI Ollama  Qdrant Chroma PGVector
  Ollama Gemini ... HuggingFace..  Pinecone FAISS ...
```

### Key Architectural Properties

| Property                 | Pattern                                 | Implementation                                                                                                  |
| ------------------------ | --------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| **Configuration**        | Pydantic models with validator chains   | `MemoryConfig` composes `LlmConfig`, `EmbedderConfig`, `VectorStoreConfig`, `GraphStoreConfig`                  |
| **Provider abstraction** | ABC + Factory                           | Each layer has a base class and a factory that maps provider strings to class paths                             |
| **Lazy loading**         | `importlib.import_module`               | Provider classes are loaded only when requested, avoiding heavy dependency trees                                |
| **Parallel execution**   | `concurrent.futures.ThreadPoolExecutor` | Vector store and graph store operations run in parallel during `add()` and `search()`                           |
| **History tracking**     | SQLite side-channel                     | Every memory mutation (ADD/UPDATE/DELETE) is logged to a SQLite `history` table, separate from the vector store |
| **Graceful degradation** | try-catch at every boundary             | Hook failures, graph store failures, and telemetry failures never block core operations                         |

### Core Class Hierarchy

```python
# mem0/memory/base.py -- The contract
class MemoryBase(ABC):
    @abstractmethod
    def get(self, memory_id): ...
    @abstractmethod
    def get_all(self): ...
    @abstractmethod
    def update(self, memory_id, data): ...
    @abstractmethod
    def delete(self, memory_id): ...
    @abstractmethod
    def history(self, memory_id): ...

# mem0/memory/main.py -- The implementation
class Memory(MemoryBase):
    def __init__(self, config: MemoryConfig = MemoryConfig()):
        self.embedding_model = EmbedderFactory.create(...)
        self.vector_store = VectorStoreFactory.create(...)
        self.llm = LlmFactory.create(...)
        self.db = SQLiteManager(...)  # History tracking
        self.graph = GraphStoreFactory.create(...)  # Optional
```

---

## 2. Memory Hierarchy

Mem0 implements a **three-level memory hierarchy** scoped by identity filters. Every operation requires at least one scope identifier.

### Memory Scopes

| Scope              | Filter Key | Typical Lifetime           | Use Case                                                      |
| ------------------ | ---------- | -------------------------- | ------------------------------------------------------------- |
| **User memory**    | `user_id`  | Persistent (cross-session) | Personal preferences, biographical facts, long-term context   |
| **Agent memory**   | `agent_id` | Persistent (per-agent)     | Agent personality, capabilities, learned behaviors            |
| **Session memory** | `run_id`   | Ephemeral (single run)     | In-conversation context, task state, multi-agent coordination |

### Multi-Scope Filtering

Scopes compose -- you can filter by any combination:

```python
# User + Agent: "What does this agent know about this user?"
memory.search(query, user_id="alice", agent_id="support-bot")

# Agent + Run: "What did this agent learn during this task?"
memory.search(query, agent_id="researcher", run_id="task-42")

# All three: Maximally specific
memory.add(messages, user_id="alice", agent_id="support-bot", run_id="session-1")
```

### Memory Types (Enum)

```python
class MemoryType(Enum):
    SEMANTIC = "semantic_memory"      # Facts, preferences, knowledge
    EPISODIC = "episodic_memory"      # Events, conversations, experiences
    PROCEDURAL = "procedural_memory"  # Step-by-step procedures, workflows
```

**Pattern:** The default `add()` creates semantic/episodic memories via fact extraction. Procedural memory requires explicit `memory_type="procedural_memory"` and uses a different LLM prompt that preserves verbatim agent action sequences.

### Actor Resolution

Within a memory scope, mem0 tracks **who said what** via an actor system:

```python
# Actor precedence for queries:
# 1. Explicit actor_id parameter
# 2. actor_id in filters dict
# 3. No actor filtering (returns all actors in scope)

# Actor identification for storage:
# Derived from message "name" field or "role" field
per_msg_meta["actor_id"] = message_dict.get("name")
per_msg_meta["role"] = message_dict["role"]
```

---

## 3. Fact Extraction Pipeline

This is the core intelligence of mem0 -- converting unstructured conversation into structured, searchable memory facts. It is a **two-LLM-call pipeline**.

### Pipeline Flow

```
Input Messages (str | list[dict])
         |
    [1] Parse & Normalize
         |  - String -> [{"role": "user", "content": ...}]
         |  - Vision messages -> text descriptions
         |  - System messages -> preserved but not extracted from
         |
    [2] LLM Call #1: Fact Extraction
         |  Input: Formatted conversation text
         |  Output: {"facts": ["Fact 1", "Fact 2", ...]}
         |
    [3] Embed Each Fact
         |  Each extracted fact is independently embedded
         |
    [4] Search Existing Memories
         |  For each new fact, search vector store for top-5 similar existing memories
         |  Scoped by user_id/agent_id/run_id filters
         |
    [5] UUID Mapping
         |  Map real UUIDs to sequential integers (0, 1, 2...)
         |  Prevents LLM from hallucinating new UUIDs
         |
    [6] LLM Call #2: Memory Management
         |  Input: Old memories + new facts
         |  Output: {"memory": [{"id": "0", "text": "...", "event": "ADD|UPDATE|DELETE|NONE"}]}
         |
    [7] Execute Actions
         |  ADD -> create new vector + history entry
         |  UPDATE -> update vector + payload + history entry
         |  DELETE -> remove vector + history entry
         |  NONE -> no-op (optionally update session IDs)
         |
    Result: List of memory operations performed
```

### LLM Call #1: Fact Extraction Prompt

Mem0 uses **role-aware extraction** -- different prompts for user memories vs. agent memories:

```python
def _should_use_agent_memory_extraction(self, messages, metadata):
    """Use agent extraction when agent_id present AND assistant messages exist."""
    has_agent_id = metadata.get("agent_id") is not None
    has_assistant_messages = any(msg.get("role") == "assistant" for msg in messages)
    return has_agent_id and has_assistant_messages
```

**User Memory Extraction Prompt** (simplified):

```
You are a Personal Information Organizer. Extract relevant facts from USER messages only.
DO NOT include information from assistant or system messages.

Types to remember:
1. Personal preferences (likes, dislikes)
2. Important personal details (names, relationships, dates)
3. Plans and intentions
4. Activity preferences
5. Health/wellness info
6. Professional details
7. Miscellaneous (books, movies, brands)

Output format: {"facts": ["Fact 1", "Fact 2"]}
```

**Agent Memory Extraction Prompt** (simplified):

```
You are an Assistant Information Organizer. Extract facts about the ASSISTANT only.
DO NOT include information from user or system messages.

Types to remember:
1. Assistant preferences
2. Capabilities
3. Personality traits
4. Approach to tasks
5. Knowledge areas

Output format: {"facts": ["Fact 1", "Fact 2"]}
```

**Key pattern:** Both prompts include few-shot examples that demonstrate extracting facts from conversations where the "wrong" party's information should be ignored. This prevents cross-contamination between user and agent memories.

### LLM Call #2: Memory Management Prompt

The second LLM call acts as a **memory manager** that decides what to do with each new fact relative to existing memories:

```
You are a smart memory manager. You can perform four operations:
1. ADD: New information not present in memory
2. UPDATE: Existing memory needs refinement (more detail, changed info)
3. DELETE: New fact contradicts existing memory
4. NONE: Fact already captured, no change needed

Guidelines:
- UPDATE when new fact has MORE information than existing
  (e.g., "Likes cricket" -> "Loves playing cricket with friends on weekends")
- Do NOT update when facts convey the same meaning
  (e.g., "Likes cheese pizza" == "Loves cheese pizza")
- DELETE when new fact CONTRADICTS existing
  (e.g., "Loves pizza" + "Dislikes pizza" -> DELETE old)
```

**Output format:**

```json
{
  "memory": [
    { "id": "0", "text": "Updated fact text", "event": "UPDATE", "old_memory": "Previous text" },
    { "id": "1", "text": "Existing fact", "event": "NONE" },
    { "id": "2", "text": "New fact", "event": "ADD" }
  ]
}
```

### UUID Hallucination Prevention

A subtle but critical pattern -- LLMs tend to hallucinate realistic-looking UUIDs, so mem0 maps them:

```python
# Before sending to LLM: map UUID -> integer
temp_uuid_mapping = {}
for idx, item in enumerate(retrieved_old_memory):
    temp_uuid_mapping[str(idx)] = item["id"]  # "0" -> "a1b2c3d4-..."
    retrieved_old_memory[idx]["id"] = str(idx)  # Present as "0", "1", "2"

# After LLM response: map integer -> UUID
memory_id = temp_uuid_mapping[resp.get("id")]  # "0" -> "a1b2c3d4-..."
```

### Raw Memory Mode (infer=False)

When `infer=False`, the extraction pipeline is bypassed entirely:

```python
if not infer:
    # Skip LLM calls -- store messages directly as memories
    for message_dict in messages:
        msg_embeddings = self.embedding_model.embed(msg_content, "add")
        mem_id = self._create_memory(msg_content, msg_embeddings, per_msg_meta)
```

This is useful for bulk import, transcript storage, or when the caller has already extracted facts.

---

## 4. Embedding Strategy

### Provider Abstraction

```python
# mem0/embeddings/base.py
class EmbeddingBase(ABC):
    def __init__(self, config: Optional[BaseEmbedderConfig] = None):
        if config is None:
            self.config = BaseEmbedderConfig()
        else:
            self.config = config

    @abstractmethod
    def embed(self, text, memory_action: Optional[Literal["add", "search", "update"]]):
        """Returns: list[float] -- the embedding vector."""
        pass
```

**Key pattern: `memory_action` parameter.** The embedding interface accepts a `memory_action` hint ("add", "search", "update") that allows providers to use different embedding types for different operations. This is used by providers like Vertex AI that support task-specific embedding variants.

### Supported Providers (11 total)

| Provider           | Default Model                            | Default Dims | Notes                              |
| ------------------ | ---------------------------------------- | ------------ | ---------------------------------- |
| `openai` (default) | `text-embedding-3-small`                 | 1536         | Supports custom dimensions via API |
| `ollama`           | `nomic-embed-text`                       | 768          | Local, privacy-first               |
| `huggingface`      | `sentence-transformers/all-MiniLM-L6-v2` | 384          | Self-hosted                        |
| `azure_openai`     | --                                       | --           | Enterprise Azure deployment        |
| `gemini`           | --                                       | --           | Google AI                          |
| `vertexai`         | --                                       | --           | GCP, supports task-specific types  |
| `together`         | --                                       | --           | Together AI platform               |
| `lmstudio`         | --                                       | --           | Local via LM Studio                |
| `langchain`        | --                                       | --           | LangChain wrapper                  |
| `aws_bedrock`      | --                                       | --           | AWS managed                        |
| `fastembed`        | --                                       | --           | Lightweight local embeddings       |

### OpenAI Embedding Implementation (Reference)

```python
class OpenAIEmbedding(EmbeddingBase):
    def __init__(self, config):
        super().__init__(config)
        self.config.model = self.config.model or "text-embedding-3-small"
        self.config.embedding_dims = self.config.embedding_dims or 1536

        api_key = self.config.api_key or os.getenv("OPENAI_API_KEY")
        base_url = self.config.openai_base_url or os.getenv("OPENAI_BASE_URL") or "https://api.openai.com/v1"
        self.client = OpenAI(api_key=api_key, base_url=base_url)

    def embed(self, text, memory_action=None):
        text = text.replace("\n", " ")
        return (
            self.client.embeddings.create(
                input=[text],
                model=self.config.model,
                dimensions=self.config.embedding_dims
            )
            .data[0]
            .embedding
        )
```

### Embedding Caching Pattern

During the `add()` pipeline, mem0 pre-computes embeddings for all extracted facts and caches them in a dict to avoid redundant embedding calls:

```python
new_message_embeddings = {}
for new_mem in new_retrieved_facts:
    messages_embeddings = self.embedding_model.embed(new_mem, "add")
    new_message_embeddings[new_mem] = messages_embeddings  # Cache by text content

# Later, in _create_memory:
def _create_memory(self, data, existing_embeddings, metadata=None):
    if data in existing_embeddings:
        embeddings = existing_embeddings[data]  # Cache hit
    else:
        embeddings = self.embedding_model.embed(data, memory_action="add")  # Cache miss
```

---

## 5. Vector Store Abstraction

### Base Interface

```python
class VectorStoreBase(ABC):
    @abstractmethod
    def create_col(self, name, vector_size, distance): ...
    @abstractmethod
    def insert(self, vectors, payloads=None, ids=None): ...
    @abstractmethod
    def search(self, query, vectors, limit=5, filters=None): ...
    @abstractmethod
    def delete(self, vector_id): ...
    @abstractmethod
    def update(self, vector_id, vector=None, payload=None): ...
    @abstractmethod
    def get(self, vector_id): ...
    @abstractmethod
    def list(self, filters=None, limit=None): ...
    @abstractmethod
    def list_cols(self): ...
    @abstractmethod
    def delete_col(self): ...
    @abstractmethod
    def col_info(self): ...
    @abstractmethod
    def reset(self): ...
```

### Supported Providers (19 total)

| Category             | Providers                                                                        |
| -------------------- | -------------------------------------------------------------------------------- |
| **Managed cloud**    | Pinecone, Upstash Vector, Azure AI Search, S3 Vectors, MongoDB Atlas, Databricks |
| **Self-hosted**      | Qdrant (default), Chroma, Milvus, Weaviate, Redis, Elasticsearch, OpenSearch     |
| **PostgreSQL-based** | PGVector, Supabase                                                               |
| **In-process**       | FAISS                                                                            |
| **GCP**              | Vertex AI Vector Search                                                          |
| **Wrapper**          | LangChain (delegates to any LangChain vector store)                              |
| **Regional**         | Baidu, Cassandra, Neptune Analytics                                              |

### Factory Pattern

```python
class VectorStoreFactory:
    provider_to_class = {
        "qdrant": "mem0.vector_stores.qdrant.Qdrant",
        "chroma": "mem0.vector_stores.chroma.ChromaDB",
        "pgvector": "mem0.vector_stores.pgvector.PGVector",
        # ... 19 providers total
    }

    @classmethod
    def create(cls, provider_name, config):
        class_type = cls.provider_to_class.get(provider_name)
        if class_type:
            vector_store_instance = load_class(class_type)  # importlib lazy load
            return vector_store_instance(**config)
        else:
            raise ValueError(f"Unsupported VectorStore provider: {provider_name}")
```

### Qdrant Implementation (Reference -- Default Provider)

Key patterns from the Qdrant implementation that other providers follow:

```python
class Qdrant(VectorStoreBase):
    def __init__(self, collection_name, embedding_model_dims, ...):
        # Auto-create collection if missing
        self.create_col(embedding_model_dims, on_disk)

    def create_col(self, vector_size, on_disk, distance=Distance.COSINE):
        # Idempotent: skip if exists
        response = self.list_cols()
        for collection in response.collections:
            if collection.name == self.collection_name:
                return
        self.client.create_collection(...)

    def _create_filter_indexes(self):
        """Index commonly filtered fields for performance."""
        common_fields = ["user_id", "agent_id", "run_id", "actor_id"]
        for field in common_fields:
            self.client.create_payload_index(
                collection_name=self.collection_name,
                field_name=field,
                field_schema="keyword"
            )

    def search(self, query, vectors, limit=5, filters=None):
        query_filter = self._create_filter(filters) if filters else None
        hits = self.client.query_points(
            collection_name=self.collection_name,
            query=vectors,         # Vector similarity search
            query_filter=query_filter,  # Metadata filtering
            limit=limit,
        )
        return hits.points
```

### Metadata Filter Translation

Mem0 translates a universal filter format into provider-specific queries:

```python
# Universal filter format (from Memory.search())
filters = {
    "user_id": "alice",                    # Exact match
    "score": {"gte": 0.8},                # Comparison operator
    "category": {"in": ["work", "personal"]},  # In-list
    "AND": [{"key1": "val1"}, {"key2": "val2"}],  # Logical AND
    "OR": [{"key1": "val1"}, {"key2": "val2"}],   # Logical OR
}

# Qdrant translation (in _create_filter):
# Each key-value -> FieldCondition with MatchValue
# Range values -> FieldCondition with Range
# Combined with Filter(must=[...])
```

---

## 6. LLM Provider Abstraction

### Base Interface

```python
class LLMBase(ABC):
    def __init__(self, config: Optional[Union[BaseLlmConfig, Dict]] = None):
        if config is None:
            self.config = BaseLlmConfig()
        elif isinstance(config, dict):
            self.config = BaseLlmConfig(**config)
        else:
            self.config = config
        self._validate_config()

    @abstractmethod
    def generate_response(
        self,
        messages: List[Dict[str, str]],
        tools: Optional[List[Dict]] = None,
        tool_choice: str = "auto",
        **kwargs
    ):
        """Returns: str or dict (if tools are used)."""
        pass

    def _get_common_params(self, **kwargs) -> Dict:
        return {
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "top_p": self.config.top_p,
            **kwargs,
        }
```

### Supported Providers (17 total)

| Provider            | Class                 | Default Model  | Notes                                  |
| ------------------- | --------------------- | -------------- | -------------------------------------- |
| `openai` (default)  | `OpenAILLM`           | `gpt-4.1-nano` | Supports OpenRouter, structured output |
| `anthropic`         | `AnthropicLLM`        | --             | Claude models                          |
| `gemini`            | `GeminiLLM`           | --             | Google Gemini                          |
| `ollama`            | `OllamaLLM`           | --             | Local models                           |
| `groq`              | `GroqLLM`             | --             | LPU inference                          |
| `together`          | `TogetherLLM`         | --             | Open-source models                     |
| `aws_bedrock`       | `AWSBedrockLLM`       | --             | AWS managed                            |
| `azure_openai`      | `AzureOpenAILLM`      | --             | Enterprise Azure                       |
| `litellm`           | `LiteLLM`             | --             | Universal proxy                        |
| `deepseek`          | `DeepSeekLLM`         | --             | Reasoning models                       |
| `xai`               | `XAILLM`              | --             | xAI models                             |
| `lmstudio`          | `LMStudioLLM`         | --             | Local server                           |
| `vllm`              | `VllmLLM`             | --             | High-perf inference                    |
| `langchain`         | `LangchainLLM`        | --             | LangChain wrapper                      |
| `openai_structured` | `OpenAIStructuredLLM` | --             | Structured output mode                 |

### Factory with Config Conversion

The `LlmFactory` handles config type conversion -- a pattern worth noting:

```python
class LlmFactory:
    provider_to_class = {
        "openai": ("mem0.llms.openai.OpenAILLM", OpenAIConfig),
        "anthropic": ("mem0.llms.anthropic.AnthropicLLM", AnthropicConfig),
        # Each provider has (class_path, config_class)
    }

    @classmethod
    def create(cls, provider_name, config=None, **kwargs):
        class_type, config_class = cls.provider_to_class[provider_name]
        llm_class = load_class(class_type)

        # Config conversion chain:
        # None -> create default config_class
        # dict -> config_class(**dict)
        # BaseLlmConfig -> convert to provider-specific config
        # Already correct type -> pass through

        return llm_class(config)

    @classmethod
    def register_provider(cls, name, class_path, config_class=None):
        """Runtime extension point for custom providers."""
        cls.provider_to_class[name] = (class_path, config_class or BaseLlmConfig)
```

### Reasoning Model Awareness

The base LLM class filters parameters for reasoning models that do not support `temperature`/`top_p`:

```python
def _is_reasoning_model(self, model: str) -> bool:
    reasoning_models = {"o1", "o1-preview", "o3-mini", "o3", "gpt-5", "gpt-5o", ...}
    return model.lower() in reasoning_models

def _get_supported_params(self, **kwargs) -> Dict:
    if self._is_reasoning_model(self.config.model):
        # Only pass messages, response_format, tools -- skip temperature/top_p
        return {k: v for k, v in kwargs.items() if k in ("messages", "response_format", "tools", "tool_choice")}
    else:
        return self._get_common_params(**kwargs)
```

---

## 7. Search & Retrieval

### Search Pipeline

```
Query (str)
     |
[1] Build filters from user_id/agent_id/run_id + custom filters
     |
[2] Embed query text
     |  embeddings = self.embedding_model.embed(query, "search")
     |
[3] Vector similarity search (parallel with graph search if enabled)
     |  memories = self.vector_store.search(
     |      query=query,
     |      vectors=embeddings,
     |      limit=limit,
     |      filters=effective_filters
     |  )
     |
[4] Score threshold filtering (optional)
     |  if threshold and mem.score < threshold: skip
     |
[5] Reranking (optional, if reranker configured)
     |  reranked = self.reranker.rerank(query, memories, limit)
     |
[6] Format results
     |  MemoryItem(id, memory, hash, score, timestamps, metadata)
     |
Result: {"results": [...], "relations": [...]}  # relations only if graph enabled
```

### Result Format

```python
# Each memory result contains:
{
    "id": "uuid-string",
    "memory": "The extracted fact text",
    "hash": "md5-of-memory-text",
    "score": 0.87,              # Cosine similarity score
    "created_at": "ISO-8601",
    "updated_at": "ISO-8601",
    "user_id": "alice",         # Promoted from payload
    "agent_id": "support-bot",  # Promoted from payload
    "metadata": {               # Any additional payload fields
        "custom_key": "value"
    }
}
```

### Reranker Integration

Mem0 supports an optional reranking step after vector search:

```python
# Supported rerankers: Cohere, SentenceTransformer, ZeroEntropy, LLM-based, HuggingFace
if rerank and self.reranker and original_memories:
    try:
        reranked_memories = self.reranker.rerank(query, original_memories, limit)
        original_memories = reranked_memories
    except Exception as e:
        logger.warning(f"Reranking failed, using original results: {e}")
        # Graceful fallback to unreranked results
```

### Enhanced Metadata Filtering

Beyond simple key-value matching, mem0 supports rich filter operators:

```python
# Operator support:
# eq, ne, gt, gte, lt, lte    -- comparison
# in, nin                      -- set membership
# contains, icontains          -- text matching
# AND, OR, NOT                 -- logical composition
# "*"                          -- wildcard (any value)

results = memory.search(
    "technical skills",
    user_id="alice",
    filters={
        "AND": [
            {"category": {"in": ["technical", "professional"]}},
            {"score": {"gte": 0.7}}
        ]
    }
)
```

---

## 8. Application to Impulse Phase 2

### Pattern Mapping

| Mem0 Pattern                      | Impulse Phase 2 Equivalent       | Adaptation Notes                                                                                                                                                                                               |
| --------------------------------- | -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Two-LLM-call pipeline**         | SessionEnd extraction            | Impulse already plans 1 LLM call at SessionEnd. Mem0 shows the value of a second call for deduplication/merging. Consider: extract facts in call #1, deduplicate against GENOME.md in call #2.                 |
| **Role-aware extraction prompts** | Agent-aware extraction           | Impulse operates in a coding context. Adapt the extraction prompt categories from "personal preferences" to: decisions, patterns, tool preferences, architectural choices, debugging learnings.                |
| **UUID hallucination prevention** | ID mapping in dedup prompt       | Critical pattern to adopt. When presenting existing GENOME entries to the LLM for merge decisions, map real IDs to integers.                                                                                   |
| **Vector similarity for dedup**   | FTS5 similarity scoring          | Phase 1 uses FTS5 text search. Phase 2 should add vector embeddings for semantic dedup. The existing FTS5 `rank` score is a useful initial signal, but cosine similarity catches paraphrases that FTS5 misses. |
| **Embedding caching**             | Cache within extraction pipeline | Pre-compute embeddings for all extracted facts before searching, then reuse during storage. Saves API calls.                                                                                                   |
| **Memory types enum**             | GENOME entry categories          | Map to Impulse's three files: GENOME.md (semantic/episodic), LIVE_STATE.json (session), HISTORY_INDEX.md (episodic). Consider adding a `type` field to GENOME entries.                                         |
| **Actor/scope filtering**         | Multi-agent awareness            | Impulse's LIVE_STATE.json already tracks agent IDs. Extend GENOME.md entries with `discovered_by: agent_id` metadata for attribution.                                                                          |
| **Factory + ABC pattern**         | Provider abstraction             | Impulse currently targets Claude Code only. For Phase 1.5+ multi-tool support, adopt the factory pattern for hook adapters.                                                                                    |
| **Graceful degradation**          | Hook failure isolation           | Already a Impulse principle (hooks never block agents). Mem0 validates this at scale.                                                                                                                          |
| **History tracking**              | HISTORY_INDEX.md                 | Impulse already has append-only history. Mem0's SQLite approach with structured fields (old_memory, new_memory, event) is richer. Consider JSONL for structured history entries.                               |
| **Reranking**                     | Not needed Phase 2               | Premature for Impulse's scale. Revisit when GENOME.md exceeds ~500 entries.                                                                                                                                    |
| **Graph memory**                  | Not needed Phase 2               | Entity relationship tracking is valuable but adds significant complexity. Revisit for Phase 3+ if decision/pattern graphs emerge as a need.                                                                    |

### Recommended Phase 2 Extraction Prompt (Adapted from Mem0)

```
You are a Codebase Knowledge Organizer for an AI coding agent. Extract relevant
decisions, patterns, and learnings from the session transcript below.

Types of information to extract:

1. **Architectural Decisions**: Technology choices, pattern selections, tradeoffs discussed
2. **Code Patterns**: Recurring approaches, preferred libraries, naming conventions
3. **Debugging Learnings**: Root causes found, failure modes discovered, workarounds
4. **Tool Preferences**: CLI flags, editor settings, workflow shortcuts
5. **Project Conventions**: File organization, testing patterns, commit styles
6. **Resolved Debates**: Questions that were settled with a clear conclusion

DO NOT extract:
- Routine operations (file reads, linting, basic git)
- Transient task state (current errors being debugged)
- Information already present in the existing knowledge base

Output: {"facts": ["Decision/pattern/learning 1", "Decision/pattern/learning 2"]}
```

### Implementation Priority

1. **P0 (Phase 2 MVP):** Adapt the two-call extraction pipeline with UUID mapping
2. **P0:** Implement embedding-based dedup (FTS5 + vector similarity hybrid)
3. **P1:** Add `type` field to GENOME entries (decision, pattern, preference, learning)
4. **P1:** Implement embedding caching within extraction pipeline
5. **P2:** Add agent attribution metadata to GENOME entries
6. **P3:** Evaluate reranking when GENOME grows large
7. **P3:** Evaluate graph memory for decision relationship tracking

---

## Appendix: Key File Locations in Mem0 Source

| File                           | Purpose                                                                    |
| ------------------------------ | -------------------------------------------------------------------------- |
| `mem0/memory/main.py`          | Core `Memory` class with add/search/update/delete                          |
| `mem0/memory/base.py`          | `MemoryBase` ABC defining the contract                                     |
| `mem0/memory/utils.py`         | Message parsing, fact retrieval helpers, JSON extraction                   |
| `mem0/memory/storage.py`       | `SQLiteManager` for history tracking                                       |
| `mem0/configs/prompts.py`      | All LLM prompts (fact extraction, memory management, procedural)           |
| `mem0/configs/base.py`         | `MemoryConfig`, `MemoryItem` Pydantic models                               |
| `mem0/configs/enums.py`        | `MemoryType` enum (semantic, episodic, procedural)                         |
| `mem0/utils/factory.py`        | `LlmFactory`, `EmbedderFactory`, `VectorStoreFactory`, `GraphStoreFactory` |
| `mem0/llms/base.py`            | `LLMBase` ABC                                                              |
| `mem0/embeddings/base.py`      | `EmbeddingBase` ABC                                                        |
| `mem0/vector_stores/base.py`   | `VectorStoreBase` ABC                                                      |
| `mem0/vector_stores/qdrant.py` | Reference vector store implementation (default)                            |
| `mem0/llms/openai.py`          | Reference LLM implementation (default)                                     |
| `mem0/embeddings/openai.py`    | Reference embedding implementation (default)                               |
| `LLM.md`                       | LLM-optimized documentation with full API reference                        |

---

_Extracted from mem0 v1.0.0 on 2026-02-21. Source: `/cloned-repos/mem0/`._
