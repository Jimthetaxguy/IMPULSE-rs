#!/bin/bash
# impulse-rs Cleanup Script
# Run this from the impulse-rs directory to reclaim ~45GB of build artifacts
# Source code is safely committed in git (verify with: git log --oneline)

set -e

cd "$(dirname "$0")"

echo "=== impulse-rs Cleanup ==="
echo "Working directory: $(pwd)"
echo ""

# Safety check: verify git commit exists
if ! git log --oneline -1 > /dev/null 2>&1; then
    echo "ERROR: No git commit found. Aborting to protect source code."
    exit 1
fi

COMMIT=$(git log --oneline -1)
echo "Git commit verified: $COMMIT"
echo ""

# Show what will be deleted
echo "=== Artifacts to delete ==="
for path in target target.old target2 impulse-gui/target Impulse.app Impulse-0.1.0-macos-arm64.dmg; do
    if [ -e "$path" ]; then
        SIZE=$(du -sh "$path" 2>/dev/null | cut -f1)
        echo "  $path ($SIZE)"
    fi
done
echo ""

read -p "Delete all of the above? (y/N) " confirm
if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
    echo "Aborted."
    exit 0
fi

echo ""
echo "Deleting build artifacts..."

rm -rf target/
echo "  Deleted target/"

rm -rf target.old/
echo "  Deleted target.old/"

rm -rf target2/
echo "  Deleted target2/"

rm -rf impulse-gui/target/
echo "  Deleted impulse-gui/target/"

rm -rf Impulse.app/
echo "  Deleted Impulse.app/"

rm -f Impulse-0.1.0-macos-arm64.dmg
echo "  Deleted Impulse-0.1.0-macos-arm64.dmg"

echo ""
echo "=== Cleanup complete ==="
echo "Final folder size: $(du -sh . | cut -f1)"
echo "Git status: $(git status --short | wc -l | tr -d ' ') uncommitted changes"
echo ""
echo "Next steps:"
echo "  1. Push to GitHub: git remote add origin git@github.com:jamespustorino/impulse-rs.git && git push -u origin main"
echo "  2. Optional: remove stale files (Cargo 2.lock, 'impulse-ops 2/')"
