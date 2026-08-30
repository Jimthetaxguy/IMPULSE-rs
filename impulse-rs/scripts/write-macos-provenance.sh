#!/usr/bin/env bash
# Write the canonical source and payload manifest for an Impulse macOS bundle.
# The manifest deliberately excludes itself; the outer DMG digest and eventual
# bundle signature bind the manifest without creating a self-referential hash.

set -euo pipefail

MANIFEST_NAME="ReleaseProvenance.v1.tsv"
SOURCE_ROOT=""
VERSION=""
APP_DIR=""
TARGETS=()

usage() {
    cat <<'EOF'
Usage: write-macos-provenance.sh --source-root DIR --version VER \
       --target TARGET [--target TARGET] APP_DIR

Write Contents/Resources/ReleaseProvenance.v1.tsv after every other bundle
payload has reached its final bytes and mode. TARGET must be one of
aarch64-apple-darwin or x86_64-apple-darwin.
EOF
}

fail() {
    echo "error: $*" >&2
    exit 1
}

require_value() {
    local flag="$1"
    local value="${2:-}"
    [[ -n "$value" ]] || fail "$flag requires a value"
}

sha256_file() {
    local file_path="$1"
    local digest
    digest="$(shasum -a 256 "$file_path" | awk '{print $1}')"
    [[ ${#digest} -eq 64 && "$digest" != *[!0-9a-f]* ]] || \
        fail "could not calculate a lowercase SHA-256 digest for $file_path"
    printf '%s' "$digest"
}

portable_mode() {
    local file_path="$1"
    local mode
    if [[ "$(uname -s)" == "Darwin" ]]; then
        mode="$(stat -f '%Lp' "$file_path")"
    else
        mode="$(stat -c '%a' "$file_path")"
    fi
    case "$mode" in
        644|755) printf '0%s' "$mode" ;;
        *) fail "unsupported payload mode $mode for $file_path" ;;
    esac
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-root)
            require_value "$1" "${2:-}"
            SOURCE_ROOT="$2"
            shift 2
            ;;
        --version)
            require_value "$1" "${2:-}"
            VERSION="$2"
            shift 2
            ;;
        --target)
            require_value "$1" "${2:-}"
            TARGETS+=("$2")
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            fail "unknown flag: $1"
            ;;
        *)
            [[ -z "$APP_DIR" ]] || fail "only one APP_DIR may be supplied"
            APP_DIR="$1"
            shift
            ;;
    esac
done

[[ -n "$SOURCE_ROOT" ]] || fail "--source-root is required"
[[ -n "$VERSION" ]] || fail "--version is required"
[[ "$VERSION" =~ ^[0-9]+([.][0-9]+){2}$ ]] || \
    fail "bundle version must be semantic numeric x.y.z: $VERSION"
[[ ${#TARGETS[@]} -gt 0 ]] || fail "at least one --target is required"
[[ -n "$APP_DIR" ]] || fail "APP_DIR is required"
[[ -d "$SOURCE_ROOT" && ! -L "$SOURCE_ROOT" ]] || \
    fail "source root must be a non-symlink directory: $SOURCE_ROOT"
[[ -d "$APP_DIR" && ! -L "$APP_DIR" ]] || \
    fail "app bundle must be a non-symlink directory: $APP_DIR"

SOURCE_ROOT="$(cd "$SOURCE_ROOT" && pwd -P)"
APP_DIR="$(cd "$APP_DIR" && pwd -P)"
REPOSITORY_ROOT="$(git -C "$SOURCE_ROOT" rev-parse --show-toplevel 2>/dev/null)" || \
    fail "source root is not inside a Git worktree"
REPOSITORY_ROOT="$(cd "$REPOSITORY_ROOT" && pwd -P)"
WORKSPACE_PREFIX="$(git -C "$SOURCE_ROOT" rev-parse --show-prefix)"
WORKSPACE_LABEL="${WORKSPACE_PREFIX%/}"
if [[ -z "$WORKSPACE_LABEL" ]]; then
    WORKSPACE_LABEL="."
fi

SOURCE_OBJECT_FORMAT="$(git -C "$REPOSITORY_ROOT" rev-parse --show-object-format)"
SOURCE_COMMIT="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{commit}')"
SOURCE_TREE="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{tree}')"
case "$SOURCE_OBJECT_FORMAT" in
    sha1) OBJECT_ID_LENGTH=40 ;;
    sha256) OBJECT_ID_LENGTH=64 ;;
    *) fail "unsupported Git object format: $SOURCE_OBJECT_FORMAT" ;;
esac
for object_id in "$SOURCE_COMMIT" "$SOURCE_TREE"; do
    [[ ${#object_id} -eq $OBJECT_ID_LENGTH && "$object_id" != *[!0-9a-f]* ]] || \
        fail "Git returned an invalid $SOURCE_OBJECT_FORMAT object id"
done

LOCK_FILE="$SOURCE_ROOT/Cargo.lock"
LOCK_RELATIVE="${WORKSPACE_PREFIX}Cargo.lock"
[[ -f "$LOCK_FILE" && ! -L "$LOCK_FILE" && -s "$LOCK_FILE" ]] || \
    fail "source workspace requires a non-empty regular Cargo.lock"
git -C "$REPOSITORY_ROOT" ls-files --error-unmatch -- "$LOCK_RELATIVE" >/dev/null 2>&1 || \
    fail "Cargo.lock must be tracked at $LOCK_RELATIVE"
git -C "$REPOSITORY_ROOT" diff --quiet HEAD -- "$LOCK_RELATIVE" || \
    fail "Cargo.lock differs from the source commit"
LOCK_DIGEST="$(sha256_file "$LOCK_FILE")"
SOURCE_STATUS="$(git -C "$REPOSITORY_ROOT" status --porcelain=v1 --untracked-files=all)"
[[ -z "$SOURCE_STATUS" ]] || fail "source-bound provenance requires a clean Git worktree"

HAS_ARM64=false
HAS_X86_64=false
for target in "${TARGETS[@]}"; do
    case "$target" in
        aarch64-apple-darwin)
            $HAS_ARM64 && fail "duplicate target: $target"
            HAS_ARM64=true
            ;;
        x86_64-apple-darwin)
            $HAS_X86_64 && fail "duplicate target: $target"
            HAS_X86_64=true
            ;;
        *) fail "unsupported macOS target: $target" ;;
    esac
done
CANONICAL_TARGETS=()
$HAS_ARM64 && CANONICAL_TARGETS+=("aarch64-apple-darwin")
$HAS_X86_64 && CANONICAL_TARGETS+=("x86_64-apple-darwin")

PAYLOAD_PATHS=(
    "Contents/Info.plist"
    "Contents/MacOS/impulse-desktop"
    "Contents/MacOS/impulse-rs"
    "Contents/MacOS/ion"
    "Contents/Resources/ReleaseCandidateNotice.txt"
    "Contents/Resources/assets/impulse_crt.css"
    "Contents/Resources/assets/vendor/xterm/LICENSE.addon-fit.txt"
    "Contents/Resources/assets/vendor/xterm/LICENSE.xterm.txt"
    "Contents/Resources/assets/vendor/xterm/addon-fit.js"
    "Contents/Resources/assets/vendor/xterm/manifest.json"
    "Contents/Resources/assets/vendor/xterm/xterm.css"
    "Contents/Resources/assets/vendor/xterm/xterm.js"
)

for relative in "${PAYLOAD_PATHS[@]}"; do
    payload="$APP_DIR/$relative"
    [[ -f "$payload" && ! -L "$payload" && -s "$payload" ]] || \
        fail "required provenance payload is missing: $relative"
done

MANIFEST_RELATIVE="Contents/Resources/$MANIFEST_NAME"
MANIFEST_PATH="$APP_DIR/$MANIFEST_RELATIVE"
[[ ! -e "$MANIFEST_PATH" && ! -L "$MANIFEST_PATH" ]] || \
    fail "provenance manifest already exists: $MANIFEST_PATH"
MANIFEST_TEMP="$(mktemp "$MANIFEST_PATH.tmp.XXXXXX")"

{
    printf 'IMPULSE_RELEASE_PROVENANCE_V1\n'
    printf 'source_object_format\t%s\n' "$SOURCE_OBJECT_FORMAT"
    printf 'source_commit\t%s\n' "$SOURCE_COMMIT"
    printf 'source_tree\t%s\n' "$SOURCE_TREE"
    printf 'source_workspace\t%s\n' "$WORKSPACE_LABEL"
    printf 'cargo_lock\t%s\tsha256:%s\n' "$LOCK_RELATIVE" "$LOCK_DIGEST"
    printf 'bundle_version\t%s\n' "$VERSION"
    printf 'build_profile\trelease\n'
    for target in "${CANONICAL_TARGETS[@]}"; do
        printf 'target\t%s\n' "$target"
    done
    printf 'inventory_exclusion\t%s\tself\n' "$MANIFEST_RELATIVE"
    for relative in "${PAYLOAD_PATHS[@]}"; do
        payload="$APP_DIR/$relative"
        mode="$(portable_mode "$payload")"
        case "$relative" in
            Contents/MacOS/*)
                [[ "$mode" == "0755" ]] || fail "executable payload mode must be 0755: $relative"
                ;;
            *)
                [[ "$mode" == "0644" ]] || fail "resource payload mode must be 0644: $relative"
                ;;
        esac
        size="$(wc -c < "$payload" | tr -d '[:space:]')"
        [[ "$size" =~ ^[0-9]+$ ]] || fail "could not calculate payload size: $relative"
        digest="$(sha256_file "$payload")"
        printf 'file\t%s\t%s\t%s\tsha256:%s\n' \
            "$relative" "$mode" "$size" "$digest"
    done
} > "$MANIFEST_TEMP"

chmod 0644 "$MANIFEST_TEMP"
mv "$MANIFEST_TEMP" "$MANIFEST_PATH"
echo "==> Wrote source-bound package provenance: $MANIFEST_PATH"
