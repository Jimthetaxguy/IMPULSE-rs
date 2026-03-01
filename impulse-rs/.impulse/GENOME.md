{
  "decisions": [
    {
      "date": "2026-02-24T12:00:00Z",
      "description": "Use pure Rust stack — no webview, no Electron",
      "rationale": "Native performance, single binary distribution",
      "tags": ["architecture"]
    },
    {
      "date": "2026-02-25T18:00:00Z",
      "description": "Atomic file I/O everywhere (temp + rename)",
      "rationale": "Prevents corrupt writes on crash or power loss",
      "tags": ["reliability"]
    }
  ],
  "preferences": [],
  "constraints": [],
  "last_updated": "2026-02-27T00:00:00Z"
}
