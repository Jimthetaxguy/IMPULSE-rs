---
name: warn-git-clean
enabled: true
event: bash
pattern: git\s+clean\s+-[a-zA-Z]*f|git\s+checkout\s+--\s+\.|git\s+reset\s+--hard
action: block
---

**BLOCKED: Destructive git operation**

These commands destroy uncommitted work. Use `git stash` to save changes first.
