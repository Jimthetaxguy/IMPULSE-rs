---
name: warn-secrets-in-code
enabled: true
event: file
conditions:
  - field: file_path
    operator: not_contains
    pattern: .env
  - field: new_text
    operator: regex_match
    pattern: (sk-[a-zA-Z0-9]{20,}|ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9_]{82}|xoxb-[0-9]{10,})
action: block
---

**BLOCKED: API key detected in source code**

Store secrets in `.env` files (gitignored), not in source code.
