#!/usr/bin/env bash
# Launch the ignored Rust acceptance harness against a read-only mounted app.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_PATH=""
SOURCE_ROOT=""
PROVENANCE_SHA256=""
PROVENANCE_MANIFEST=""
SOURCE_SNAPSHOT=""
SOURCE_SNAPSHOT_PARENT=""
SNAPSHOT_ADD_ATTEMPTED=false

usage() {
    cat <<'EOF'
Usage: verify-packaged-host.sh --source-root ROOT APP_DIR

Run the explicitly ignored packaged_live_host_acceptance Rust test against an
Impulse.app mounted on a read-only filesystem. The test launches the packaged
daemon and desktop under isolated state, validates a nonce-bound receipt, and
checks cleanup plus protected-state wardens.
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

canonical_dir() {
    local path="$1"
    [[ -d "$path" && ! -L "$path" ]] || fail "directory is missing or symlinked: $path"
    (cd "$path" && pwd -P)
}

canonical_existing_dir() {
    local path="$1"
    [[ -d "$path" ]] || fail "directory is missing: $path"
    (cd "$path" && pwd -P)
}

resolve_future_dir() {
    local path="$1"
    local base="$2"
    local probe suffix component parent

    if [[ "$path" != /* ]]; then
        path="$base/$path"
    fi
    while [[ "$path" == *//* ]]; do
        path="${path//\/\//\/}"
    done
    probe="${path%/}"
    [[ -n "$probe" ]] || probe="/"
    suffix=""
    while [[ ! -d "$probe" ]]; do
        [[ ! -e "$probe" && ! -L "$probe" ]] || \
            fail "Cargo target path contains a non-directory component"
        component="${probe##*/}"
        [[ -n "$component" && "$component" != "." && "$component" != ".." ]] || \
            fail "Cargo target path contains an unsafe component"
        suffix="/$component$suffix"
        parent="${probe%/*}"
        [[ -n "$parent" ]] || parent="/"
        [[ "$parent" != "$probe" ]] || fail "could not resolve Cargo target path"
        probe="$parent"
    done
    probe="$(canonical_existing_dir "$probe")"
    if [[ "$probe" == "/" ]]; then
        printf '/%s' "${suffix#/}"
    else
        printf '%s%s' "$probe" "$suffix"
    fi
}

path_is_within() {
    local candidate="$1"
    local root="$2"
    [[ "$candidate" == "$root" || "$candidate" == "$root/"* ]]
}

paths_overlap() {
    path_is_within "$1" "$2" || path_is_within "$2" "$1"
}

validate_target_dir() {
    local target="$1"
    shift
    while [[ $# -gt 0 ]]; do
        local label="$1"
        local protected="$2"
        shift 2
        paths_overlap "$target" "$protected" && \
            fail "Cargo target overlaps protected $label"
    done
    return 0
}

prepare_target_dir() {
    local requested="$1"
    local relative_base="$2"
    shift 2
    local candidate resolved
    candidate="$(resolve_future_dir "$requested" "$relative_base")"
    validate_target_dir "$candidate" "$@"
    mkdir -p "$candidate"
    resolved="$(canonical_existing_dir "$candidate")"
    validate_target_dir "$resolved" "$@"
    printf '%s' "$resolved"
}

require_target_child_path() {
    local candidate="$1"
    local target_root="$2"
    local label="$3"
    if [[ "$candidate" == "$target_root" ]] || ! path_is_within "$candidate" "$target_root"; then
        fail "$label must remain within the Cargo target root"
    fi
}

prepare_target_child_dir() {
    local requested="$1"
    local target_root="$2"
    local label="$3"
    shift 3
    local resolved
    resolved="$(prepare_target_dir "$requested" "$target_root" "$@")"
    require_target_child_path "$resolved" "$target_root" "$label"
    printf '%s' "$resolved"
}

recheck_target_dir() {
    local expected="$1"
    shift
    local resolved
    resolved="$(canonical_existing_dir "$expected")"
    [[ "$resolved" == "$expected" ]] || \
        fail "Cargo target resolution changed after creation"
    validate_target_dir "$resolved" "$@"
}

source_sha256() {
    local file_path="$1"
    local digest
    digest="$(/usr/bin/shasum -a 256 "$file_path" | /usr/bin/awk '{print $1}')"
    [[ ${#digest} -eq 64 && "$digest" != *[!0-9a-f]* ]] || \
        fail "could not calculate source SHA-256: $file_path"
    printf '%s' "$digest"
}

verify_source_unchanged() {
    local observed_commit observed_tree observed_lock observed_status
    observed_commit="$(git -C "$SOURCE_ROOT" rev-parse --verify 'HEAD^{commit}')"
    observed_tree="$(git -C "$SOURCE_ROOT" rev-parse --verify 'HEAD^{tree}')"
    observed_lock="$(source_sha256 "$SOURCE_WORKSPACE_ROOT/Cargo.lock")"
    observed_status="$(git -C "$SOURCE_ROOT" status --porcelain=v1 --untracked-files=all)"
    [[ "$observed_commit" == "$SOURCE_COMMIT" ]] || \
        fail "source commit changed during packaged-host verification"
    [[ "$observed_tree" == "$SOURCE_TREE" ]] || \
        fail "source tree changed during packaged-host verification"
    [[ "$observed_lock" == "$SOURCE_LOCK_DIGEST" ]] || \
        fail "Cargo.lock changed during packaged-host verification"
    [[ -z "$observed_status" ]] || \
        fail "source worktree changed during packaged-host verification"
}

verify_snapshot_unchanged() {
    local observed_commit observed_tree observed_lock observed_status
    observed_commit="$(git -C "$SOURCE_SNAPSHOT" rev-parse --verify 'HEAD^{commit}')"
    observed_tree="$(git -C "$SOURCE_SNAPSHOT" rev-parse --verify 'HEAD^{tree}')"
    observed_lock="$(source_sha256 "$SNAPSHOT_WORKSPACE_ROOT/Cargo.lock")"
    observed_status="$(git -C "$SOURCE_SNAPSHOT" status --porcelain=v1 --untracked-files=all)"
    [[ "$observed_commit" == "$SOURCE_COMMIT" ]] || \
        fail "detached acceptance snapshot commit changed"
    [[ "$observed_tree" == "$SOURCE_TREE" ]] || \
        fail "detached acceptance snapshot tree changed"
    [[ "$observed_lock" == "$SOURCE_LOCK_DIGEST" ]] || \
        fail "detached acceptance snapshot Cargo.lock changed"
    [[ -z "$observed_status" ]] || fail "detached acceptance snapshot is not clean"
}

cleanup_source_snapshot() {
    local status=$?
    local cleanup_failed=false
    trap - EXIT

    if $SNAPSHOT_ADD_ATTEMPTED; then
        if git -C "$SOURCE_ROOT" worktree remove "$SOURCE_SNAPSHOT"; then
            SNAPSHOT_ADD_ATTEMPTED=false
        else
            echo "error: failed to remove the clean detached acceptance snapshot; preserved for inspection: $SOURCE_SNAPSHOT" >&2
            cleanup_failed=true
        fi
    fi
    if ! $cleanup_failed && [[ -n "$SOURCE_SNAPSHOT_PARENT" && -d "$SOURCE_SNAPSHOT_PARENT" ]]; then
        if ! rmdir "$SOURCE_SNAPSHOT_PARENT"; then
            echo "error: detached acceptance snapshot parent was not empty; preserved for inspection: $SOURCE_SNAPSHOT_PARENT" >&2
            cleanup_failed=true
        fi
    fi
    if [[ $status -eq 0 ]] && $cleanup_failed; then
        status=1
    fi
    exit "$status"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-root)
            require_value "$1" "${2:-}"
            SOURCE_ROOT="$2"
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
            [[ -z "$APP_PATH" ]] || fail "only one APP_DIR may be supplied"
            APP_PATH="$1"
            shift
            ;;
    esac
done

[[ -n "$APP_PATH" ]] || fail "APP_DIR is required"
[[ -n "$SOURCE_ROOT" ]] || fail "--source-root is required"

APP_PATH="$(canonical_dir "$APP_PATH")"
SOURCE_ROOT="$(canonical_dir "$SOURCE_ROOT")"
SOURCE_GIT_ROOT="$(git -C "$SOURCE_ROOT" rev-parse --show-toplevel 2>/dev/null)" || \
    fail "--source-root is not inside a Git worktree"
SOURCE_GIT_ROOT="$(canonical_dir "$SOURCE_GIT_ROOT")"
[[ "$SOURCE_ROOT" == "$SOURCE_GIT_ROOT" ]] || \
    fail "--source-root must name an exact Git worktree root"

SCRIPT_REPOSITORY_ROOT="$(git -C "$WORKSPACE_ROOT" rev-parse --show-toplevel 2>/dev/null)" || \
    fail "packaged-host verifier is not inside a Git worktree"
SCRIPT_REPOSITORY_ROOT="$(canonical_dir "$SCRIPT_REPOSITORY_ROOT")"
WORKSPACE_PREFIX="$(git -C "$WORKSPACE_ROOT" rev-parse --show-prefix)"
if [[ -n "$WORKSPACE_PREFIX" ]]; then
    SOURCE_WORKSPACE_ROOT="$SOURCE_ROOT/${WORKSPACE_PREFIX%/}"
else
    SOURCE_WORKSPACE_ROOT="$SOURCE_ROOT"
fi
SOURCE_WORKSPACE_ROOT="$(canonical_dir "$SOURCE_WORKSPACE_ROOT")"
[[ -f "$SOURCE_WORKSPACE_ROOT/Cargo.toml" && \
    -f "$SOURCE_WORKSPACE_ROOT/Cargo.lock" && \
    ! -L "$SOURCE_WORKSPACE_ROOT/Cargo.lock" && \
    -s "$SOURCE_WORKSPACE_ROOT/Cargo.lock" ]] || \
    fail "source root does not contain the expected Cargo workspace"

SOURCE_COMMIT="$(git -C "$SOURCE_ROOT" rev-parse --verify 'HEAD^{commit}')"
SOURCE_TREE="$(git -C "$SOURCE_ROOT" rev-parse --verify 'HEAD^{tree}')"
SOURCE_LOCK_DIGEST="$(source_sha256 "$SOURCE_WORKSPACE_ROOT/Cargo.lock")"
verify_source_unchanged

SCRIPT_COMMIT="$(git -C "$SCRIPT_REPOSITORY_ROOT" rev-parse --verify 'HEAD^{commit}')"
SCRIPT_TREE="$(git -C "$SCRIPT_REPOSITORY_ROOT" rev-parse --verify 'HEAD^{tree}')"
SCRIPT_LOCK_DIGEST="$(source_sha256 "$WORKSPACE_ROOT/Cargo.lock")"
SCRIPT_STATUS="$(git -C "$SCRIPT_REPOSITORY_ROOT" status --porcelain=v1 --untracked-files=all)"
[[ "$SCRIPT_COMMIT" == "$SOURCE_COMMIT" && "$SCRIPT_TREE" == "$SOURCE_TREE" && \
    "$SCRIPT_LOCK_DIGEST" == "$SOURCE_LOCK_DIGEST" && -z "$SCRIPT_STATUS" ]] || \
    fail "packaged-host coordinator must come from the exact clean source commit"

GIT_COMMON_DIR="$(git -C "$SOURCE_ROOT" rev-parse --path-format=absolute --git-common-dir)"
CANONICAL_ROOT="$(canonical_dir "$(dirname "$GIT_COMMON_DIR")")"
[[ -n "${HOME:-}" ]] || fail "HOME is required to protect the live Impulse state"
HOME_ROOT="$(canonical_existing_dir "$HOME")"
LIVE_IMPULSE_ROOT="$(resolve_future_dir "$HOME_ROOT/.impulse" "$HOME_ROOT")"
TEMP_BASE="$(canonical_existing_dir "${TMPDIR:-/tmp}")"
validate_target_dir "$TEMP_BASE" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT"

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    REQUESTED_TARGET_BASE="$CARGO_TARGET_DIR"
else
    REQUESTED_TARGET_BASE="$TEMP_BASE/impulse-packaged-host-${UID}-${SOURCE_COMMIT}-$$"
fi
TARGET_BASE="$(prepare_target_dir "$REQUESTED_TARGET_BASE" "$SOURCE_WORKSPACE_ROOT" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT")"
ACCEPTANCE_RUNS_ROOT="$(prepare_target_child_dir "$TARGET_BASE/packaged-host-runs" "$TARGET_BASE" \
    "packaged-host run output" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT" \
    "mounted app" "$APP_PATH")"
ACCEPTANCE_TARGET_DIR="$(mktemp -d "$ACCEPTANCE_RUNS_ROOT/acceptance.XXXXXX")"
chmod 0700 "$ACCEPTANCE_TARGET_DIR"
ACCEPTANCE_TARGET_DIR="$(canonical_existing_dir "$ACCEPTANCE_TARGET_DIR")"
require_target_child_path "$ACCEPTANCE_TARGET_DIR" "$TARGET_BASE" \
    "fresh packaged-host Cargo target"
validate_target_dir "$ACCEPTANCE_TARGET_DIR" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT" \
    "mounted app" "$APP_PATH"

# All target directories are resolved and checked before either verifier or
# Cargo can execute, so their writes cannot become part of any warden root.
[[ "$(uname -s)" == "Darwin" ]] || fail "packaged desktop acceptance requires macOS"
[[ -x "$APP_PATH/Contents/MacOS/impulse-desktop" ]] || \
    fail "packaged impulse-desktop executable is missing"
[[ -x "$APP_PATH/Contents/MacOS/impulse-rs" ]] || \
    fail "packaged impulse-rs companion is missing"
PROVENANCE_MANIFEST="$APP_PATH/Contents/Resources/ReleaseProvenance.v1.tsv"
[[ -f "$PROVENANCE_MANIFEST" && ! -L "$PROVENANCE_MANIFEST" && -s "$PROVENANCE_MANIFEST" ]] || \
    fail "embedded ReleaseProvenance.v1.tsv is missing, symlinked, or empty"
PROVENANCE_SHA256="$(source_sha256 "$PROVENANCE_MANIFEST")"
[[ "$PROVENANCE_SHA256" =~ ^[0-9a-f]{64}$ ]] || \
    fail "could not derive a lowercase SHA-256 from embedded ReleaseProvenance.v1.tsv"

SOURCE_SNAPSHOT_PARENT="$(mktemp -d "$TEMP_BASE/impulse-packaged-source.XXXXXX")"
SOURCE_SNAPSHOT="$SOURCE_SNAPSHOT_PARENT/source"
SNAPSHOT_ADD_ATTEMPTED=true
trap cleanup_source_snapshot EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
git -C "$SOURCE_ROOT" worktree add --detach "$SOURCE_SNAPSHOT" "$SOURCE_COMMIT"

if [[ -n "$WORKSPACE_PREFIX" ]]; then
    SNAPSHOT_WORKSPACE_ROOT="$SOURCE_SNAPSHOT/${WORKSPACE_PREFIX%/}"
else
    SNAPSHOT_WORKSPACE_ROOT="$SOURCE_SNAPSHOT"
fi
SNAPSHOT_WORKSPACE_ROOT="$(canonical_dir "$SNAPSHOT_WORKSPACE_ROOT")"
SNAPSHOT_BUNDLE_VERIFY_SCRIPT="$SNAPSHOT_WORKSPACE_ROOT/scripts/verify-macos-app.sh"
[[ -f "$SNAPSHOT_BUNDLE_VERIFY_SCRIPT" && ! -L "$SNAPSHOT_BUNDLE_VERIFY_SCRIPT" && \
    -s "$SNAPSHOT_BUNDLE_VERIFY_SCRIPT" ]] || \
    fail "exact source snapshot is missing its bundle verifier"
[[ -f "$SNAPSHOT_WORKSPACE_ROOT/Cargo.toml" && \
    -f "$SNAPSHOT_WORKSPACE_ROOT/Cargo.lock" ]] || \
    fail "exact source snapshot is missing its Cargo manifests"
if git -C "$SOURCE_SNAPSHOT" symbolic-ref -q HEAD >/dev/null 2>&1; then
    fail "acceptance source snapshot must have a detached HEAD"
fi
validate_target_dir "$TARGET_BASE" \
    "detached source snapshot" "$SOURCE_SNAPSHOT" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT"
validate_target_dir "$ACCEPTANCE_TARGET_DIR" \
    "detached source snapshot" "$SOURCE_SNAPSHOT" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT" \
    "mounted app" "$APP_PATH"
require_target_child_path "$ACCEPTANCE_TARGET_DIR" "$TARGET_BASE" \
    "fresh packaged-host Cargo target"

verify_snapshot_unchanged
verify_source_unchanged
bash "$SNAPSHOT_BUNDLE_VERIFY_SCRIPT" --macos \
    --source-root "$SNAPSHOT_WORKSPACE_ROOT" "$APP_PATH"
verify_snapshot_unchanged
verify_source_unchanged

recheck_target_dir "$TARGET_BASE" \
    "detached source snapshot" "$SOURCE_SNAPSHOT" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT"
recheck_target_dir "$ACCEPTANCE_TARGET_DIR" \
    "detached source snapshot" "$SOURCE_SNAPSHOT" \
    "source worktree" "$SOURCE_ROOT" \
    "canonical checkout" "$CANONICAL_ROOT" \
    "live Impulse home" "$LIVE_IMPULSE_ROOT" \
    "mounted app" "$APP_PATH"
require_target_child_path "$ACCEPTANCE_TARGET_DIR" "$TARGET_BASE" \
    "fresh packaged-host Cargo target"
echo "==> Running real packaged Dioxus host acceptance"
IMPULSE_PACKAGED_APP_PATH="$APP_PATH" \
IMPULSE_PACKAGED_SOURCE_ROOT="$SOURCE_ROOT" \
IMPULSE_PACKAGED_CANONICAL_ROOT="$CANONICAL_ROOT" \
IMPULSE_PACKAGED_PROVENANCE_SHA256="$PROVENANCE_SHA256" \
CARGO_TARGET_DIR="$ACCEPTANCE_TARGET_DIR" \
cargo test \
    --locked \
    --manifest-path "$SNAPSHOT_WORKSPACE_ROOT/Cargo.toml" \
    -p impulse-desktop \
    --test packaged_live_host_acceptance \
    test_packaged_live_host_acceptance_real_mounted_app \
    -- \
    --exact \
    --ignored \
    --nocapture

verify_snapshot_unchanged
verify_source_unchanged
bash "$SNAPSHOT_BUNDLE_VERIFY_SCRIPT" --macos \
    --source-root "$SNAPSHOT_WORKSPACE_ROOT" "$APP_PATH"
verify_snapshot_unchanged
verify_source_unchanged
