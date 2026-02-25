#!/usr/bin/env python3
"""Embedding helper for impulse-rs retrieval.

Input (stdin JSON):
  {"texts": ["..."], "model": "all-MiniLM-L6-v2"}

Output (stdout JSON):
  {"vectors": [[...], [...]], "dim": 384}
"""

import hashlib
import json
import math
import os
import sys
from typing import List

DIM = 384


def deterministic_vector(text: str, dim: int = DIM) -> List[float]:
    # Stable fallback vector for environments without sentence-transformers.
    data = text.encode("utf-8")
    digest = hashlib.sha256(data).digest()
    vec = []
    for i in range(dim):
        b = digest[i % len(digest)]
        val = ((b / 255.0) * 2.0) - 1.0
        vec.append(val)

    norm = math.sqrt(sum(x * x for x in vec)) or 1.0
    return [x / norm for x in vec]


def sentence_transformers_vectors(texts: List[str], model_name: str) -> List[List[float]]:
    from sentence_transformers import SentenceTransformer  # type: ignore

    model = SentenceTransformer(model_name)
    vectors = model.encode(texts, convert_to_numpy=True, normalize_embeddings=True)
    return [v.astype("float32").tolist() for v in vectors]


def main() -> int:
    try:
        payload = json.loads(sys.stdin.read() or "{}")
    except json.JSONDecodeError as e:
        print(json.dumps({"error": f"invalid json: {e}"}), file=sys.stderr)
        return 1

    texts = payload.get("texts", [])
    model_name = payload.get("model", "all-MiniLM-L6-v2")

    if not isinstance(texts, list) or not all(isinstance(t, str) for t in texts):
        print(json.dumps({"error": "texts must be a list of strings"}), file=sys.stderr)
        return 1

    try:
        vectors = sentence_transformers_vectors(texts, model_name)
        dim = len(vectors[0]) if vectors else DIM
    except Exception as exc:
        if os.getenv("IMPULSE_EMBED_ALLOW_FAKE", "") == "1":
            vectors = [deterministic_vector(t, DIM) for t in texts]
            dim = DIM
        else:
            print(
                json.dumps(
                    {
                        "error": "sentence-transformers unavailable",
                        "detail": str(exc),
                        "hint": "install sentence-transformers or set IMPULSE_EMBED_ALLOW_FAKE=1 for development fallback",
                    }
                ),
                file=sys.stderr,
            )
            return 1

    print(json.dumps({"vectors": vectors, "dim": dim}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
