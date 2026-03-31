---
name: block-rm-rf
enabled: true
event: bash
pattern: rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|(-[a-zA-Z]*f[a-zA-Z]*r))\s|rm\s+-rf\s|rm\s+-r\s+
action: block
---

**BLOCKED: Recursive file deletion detected**

`rm -rf` is irreversible. Ask the user for explicit confirmation before deleting directories.

**Safe alternatives:**
- List first: `ls <path>`
- Move to trash: `mv <path> ~/.Trash/`
- For build artifacts only: `rm -rf node_modules/ .next/ target/ dist/`
