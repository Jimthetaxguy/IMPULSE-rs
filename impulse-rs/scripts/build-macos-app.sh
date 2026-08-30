#!/usr/bin/env bash
# Build a non-distributable Dioxus Impulse.app candidate and, optionally, a DMG.
# This script deliberately does not apply Developer ID bundle signing, notarize,
# install, tag, or publish.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DESKTOP_CRATE="$PROJECT_ROOT/impulse-desktop"
DESKTOP_RESOURCES="$DESKTOP_CRATE/resources"
VERIFY_SCRIPT="$SCRIPT_DIR/verify-macos-app.sh"
PROVENANCE_SCRIPT="$SCRIPT_DIR/write-macos-provenance.sh"
LIVE_HOST_VERIFY_SCRIPT="$SCRIPT_DIR/verify-packaged-host.sh"
PROVENANCE_FILENAME="ReleaseProvenance.v1.tsv"
APP_NAME="Impulse"

usage() {
    cat <<'EOF'
Usage: build-macos-app.sh [OPTIONS]

Build a macOS bundle containing the feature-gated Dioxus desktop host and the
impulse-rs control-plane companion plus native Ion sibling. Outputs are
non-distributable developer previews, and existing outputs are archived under
the Cargo target directory. The build requires a clean Git source tree and
embeds a source-bound ReleaseProvenance.v1.tsv payload manifest.

Options:
  --universal  Build arm64 + x86_64 Mach-O binaries with lipo
  --dmg        Create and inspect a DMG after bundle verification
  -h, --help   Show this help
EOF
}

fail() {
    echo "error: $*" >&2
    exit 1
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

archive_existing() {
    local path="$1"
    if [[ ! -e "$path" && ! -L "$path" ]]; then
        return
    fi

    local archive_dir
    archive_dir="$(mktemp -d "$ARCHIVE_ROOT/archive.XXXXXX")"
    mv "$path" "$archive_dir/$(basename "$path")"
    echo "==> Archived existing $(basename "$path") to $archive_dir"
}

package_version() {
    local manifest="$1"
    awk -F '"' '/^version[[:space:]]*=[[:space:]]*"/ { print $2; exit }' "$manifest"
}

source_sha256() {
    local file_path="$1"
    local digest
    digest="$(shasum -a 256 "$file_path" | awk '{print $1}')"
    [[ ${#digest} -eq 64 && "$digest" != *[!0-9a-f]* ]] || \
        fail "could not calculate source SHA-256: $file_path"
    printf '%s' "$digest"
}

verify_mounted_volume_root() {
    local mount_root="$1"
    local expected_name="$2"
    local entry
    local -a mounted_entries=()

    while IFS= read -r -d '' entry; do
        mounted_entries+=("$entry")
    done < <(find "$mount_root" -mindepth 1 -maxdepth 1 -print0)
    [[ ${#mounted_entries[@]} -eq 1 ]] || \
        fail "mounted DMG root must contain exactly $expected_name"
    entry="${mounted_entries[0]}"
    [[ "${entry##*/}" == "$expected_name" && -d "$entry" && ! -L "$entry" ]] || \
        fail "mounted DMG root must contain exactly one non-symlink $expected_name directory"
}

SOURCE_SNAPSHOT=""
SOURCE_SNAPSHOT_PARENT=""
SNAPSHOT_SOURCE_ROOT=""
SNAPSHOT_ADD_ATTEMPTED=false

cleanup_source_snapshot() {
    local status=$?
    local cleanup_failed=false
    trap - EXIT

    if $SNAPSHOT_ADD_ATTEMPTED; then
        if git -C "$SNAPSHOT_SOURCE_ROOT" worktree remove "$SOURCE_SNAPSHOT"; then
            SNAPSHOT_ADD_ATTEMPTED=false
        else
            echo "error: failed to remove the clean detached source snapshot; preserved for inspection: $SOURCE_SNAPSHOT" >&2
            cleanup_failed=true
        fi
    fi
    if ! $cleanup_failed && [[ -n "$SOURCE_SNAPSHOT_PARENT" && -d "$SOURCE_SNAPSHOT_PARENT" ]]; then
        if ! rmdir "$SOURCE_SNAPSHOT_PARENT"; then
            echo "error: detached source snapshot parent was not empty; preserved for inspection: $SOURCE_SNAPSHOT_PARENT" >&2
            cleanup_failed=true
        fi
    fi
    if [[ $status -eq 0 ]] && $cleanup_failed; then
        status=1
    fi
    exit "$status"
}

coordinate_exact_commit_build() {
    local protected_source_root workspace_prefix source_commit source_tree source_lock_digest
    local source_status git_common_dir canonical_root home_root live_impulse_root temp_base
    local requested_output snapshot_workspace snapshot_build_script output_name prepared_output
    local cargo_builds_root cargo_build_target build_status
    local -a snapshot_args

    protected_source_root="$(git -C "$PROJECT_ROOT" rev-parse --show-toplevel 2>/dev/null)" || \
        fail "project root is not inside a Git worktree"
    protected_source_root="$(canonical_existing_dir "$protected_source_root")"
    workspace_prefix="$(git -C "$PROJECT_ROOT" rev-parse --show-prefix)"
    source_commit="$(git -C "$protected_source_root" rev-parse --verify 'HEAD^{commit}')"
    source_tree="$(git -C "$protected_source_root" rev-parse --verify 'HEAD^{tree}')"
    [[ -f "$PROJECT_ROOT/Cargo.lock" && ! -L "$PROJECT_ROOT/Cargo.lock" && \
        -s "$PROJECT_ROOT/Cargo.lock" ]] || fail "missing regular Cargo.lock"
    source_lock_digest="$(source_sha256 "$PROJECT_ROOT/Cargo.lock")"
    source_status="$(git -C "$protected_source_root" status --porcelain=v1 --untracked-files=all)"
    [[ -z "$source_status" ]] || \
        fail "source-bound packaging requires a clean Git worktree"

    git_common_dir="$(git -C "$protected_source_root" rev-parse --path-format=absolute --git-common-dir)"
    canonical_root="$(canonical_existing_dir "$(dirname "$git_common_dir")")"
    [[ -n "${HOME:-}" ]] || fail "HOME is required to protect the live Impulse state"
    home_root="$(canonical_existing_dir "$HOME")"
    live_impulse_root="$(resolve_future_dir "$home_root/.impulse" "$home_root")"
    temp_base="$(canonical_existing_dir "${TMPDIR:-/tmp}")"
    validate_target_dir "$temp_base" \
        "source worktree" "$protected_source_root" \
        "canonical checkout" "$canonical_root" \
        "live Impulse home" "$live_impulse_root"

    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        requested_output="$CARGO_TARGET_DIR"
    else
        requested_output="$temp_base/impulse-macos-package-${UID}-${source_commit}-$$"
    fi
    OUTPUT_ROOT="$(prepare_target_dir "$requested_output" "$PROJECT_ROOT" \
        "source worktree" "$protected_source_root" \
        "canonical checkout" "$canonical_root" \
        "live Impulse home" "$live_impulse_root")"
    # Fail before snapshot creation or Cargo if any reused output descendant
    # resolves through a symlink into protected state.
    for output_name in package package-archives package-staging package-mounts cargo-builds; do
        prepared_output="$(prepare_target_child_dir "$OUTPUT_ROOT/$output_name" "$OUTPUT_ROOT" \
            "$output_name output" \
            "source worktree" "$protected_source_root" \
            "canonical checkout" "$canonical_root" \
            "live Impulse home" "$live_impulse_root")"
        [[ -n "$prepared_output" ]] || fail "could not prepare $output_name output"
    done
    cargo_builds_root="$(canonical_existing_dir "$OUTPUT_ROOT/cargo-builds")"
    cargo_build_target="$(mktemp -d "$cargo_builds_root/build.XXXXXX")"
    chmod 0700 "$cargo_build_target"
    cargo_build_target="$(canonical_existing_dir "$cargo_build_target")"
    require_target_child_path "$cargo_build_target" "$OUTPUT_ROOT" "fresh Cargo build target"
    validate_target_dir "$cargo_build_target" \
        "source worktree" "$protected_source_root" \
        "canonical checkout" "$canonical_root" \
        "live Impulse home" "$live_impulse_root"

    SOURCE_SNAPSHOT_PARENT="$(mktemp -d "$temp_base/impulse-macos-source.XXXXXX")"
    SOURCE_SNAPSHOT="$SOURCE_SNAPSHOT_PARENT/source"
    SNAPSHOT_SOURCE_ROOT="$protected_source_root"
    SNAPSHOT_ADD_ATTEMPTED=true
    trap cleanup_source_snapshot EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    git -C "$protected_source_root" worktree add --detach "$SOURCE_SNAPSHOT" "$source_commit"
    snapshot_workspace="$SOURCE_SNAPSHOT/${workspace_prefix%/}"
    snapshot_workspace="$(canonical_existing_dir "$snapshot_workspace")"
    validate_target_dir "$OUTPUT_ROOT" \
        "detached source snapshot" "$SOURCE_SNAPSHOT" \
        "source worktree" "$protected_source_root" \
        "canonical checkout" "$canonical_root" \
        "live Impulse home" "$live_impulse_root"
    validate_target_dir "$cargo_build_target" \
        "detached source snapshot" "$SOURCE_SNAPSHOT" \
        "source worktree" "$protected_source_root" \
        "canonical checkout" "$canonical_root" \
        "live Impulse home" "$live_impulse_root"
    snapshot_build_script="$snapshot_workspace/scripts/build-macos-app.sh"
    [[ -f "$snapshot_build_script" && ! -L "$snapshot_build_script" && \
        -s "$snapshot_build_script" ]] || fail "exact source snapshot is missing its build script"
    SNAPSHOT_BUILD_SCRIPT="$snapshot_build_script"

    snapshot_args=()
    $UNIVERSAL && snapshot_args+=(--universal)
    $CREATE_DMG && snapshot_args+=(--dmg)
    echo "==> Re-entering exact detached source commit $source_commit"
    if IMPULSE_MACOS_BUILD_EXACT_COMMIT="$source_commit" \
        IMPULSE_MACOS_BUILD_EXACT_TREE="$source_tree" \
        IMPULSE_MACOS_BUILD_EXACT_LOCK_SHA256="$source_lock_digest" \
        IMPULSE_MACOS_BUILD_EXPECTED_SNAPSHOT_ROOT="$SOURCE_SNAPSHOT" \
        IMPULSE_MACOS_BUILD_PROTECTED_SOURCE_ROOT="$protected_source_root" \
        IMPULSE_MACOS_BUILD_PROTECTED_CANONICAL_ROOT="$canonical_root" \
        IMPULSE_MACOS_BUILD_PROTECTED_LIVE_ROOT="$live_impulse_root" \
        IMPULSE_MACOS_BUILD_OUTPUT_ROOT="$OUTPUT_ROOT" \
        IMPULSE_MACOS_BUILD_COORDINATOR_PID="$$" \
        CARGO_TARGET_DIR="$cargo_build_target" \
            bash "$SNAPSHOT_BUILD_SCRIPT" "${snapshot_args[@]}"; then
        build_status=0
    else
        build_status=$?
    fi
    return "$build_status"
}

enter_exact_commit_build() {
    local expected_snapshot_root protected_source_root protected_canonical_root
    local protected_live_root coordinator_pid observed_workspace_prefix
    local observed_workspace_root
    local observed_commit observed_tree observed_lock observed_status output_root

    expected_snapshot_root="${IMPULSE_MACOS_BUILD_EXPECTED_SNAPSHOT_ROOT:-}"
    protected_source_root="${IMPULSE_MACOS_BUILD_PROTECTED_SOURCE_ROOT:-}"
    protected_canonical_root="${IMPULSE_MACOS_BUILD_PROTECTED_CANONICAL_ROOT:-}"
    protected_live_root="${IMPULSE_MACOS_BUILD_PROTECTED_LIVE_ROOT:-}"
    output_root="${IMPULSE_MACOS_BUILD_OUTPUT_ROOT:-}"
    coordinator_pid="${IMPULSE_MACOS_BUILD_COORDINATOR_PID:-}"
    [[ -n "$expected_snapshot_root" && -n "$protected_source_root" && \
        -n "$protected_canonical_root" && -n "$protected_live_root" && \
        -n "$coordinator_pid" && -n "$output_root" && -n "${CARGO_TARGET_DIR:-}" ]] || \
        fail "incomplete exact-commit build coordinator state"
    [[ "$coordinator_pid" =~ ^[0-9]+$ && "$coordinator_pid" == "$PPID" ]] || \
        fail "exact-commit build must be invoked by its source-snapshot coordinator"

    expected_snapshot_root="$(canonical_existing_dir "$expected_snapshot_root")"
    REPOSITORY_ROOT="$(git -C "$PROJECT_ROOT" rev-parse --show-toplevel 2>/dev/null)" || \
        fail "snapshot project root is not inside a Git worktree"
    REPOSITORY_ROOT="$(canonical_existing_dir "$REPOSITORY_ROOT")"
    [[ "$REPOSITORY_ROOT" == "$expected_snapshot_root" ]] || \
        fail "build script is not executing from the expected detached source snapshot"
    if git -C "$REPOSITORY_ROOT" symbolic-ref -q HEAD >/dev/null 2>&1; then
        fail "exact source snapshot must have a detached HEAD"
    fi

    protected_source_root="$(canonical_existing_dir "$protected_source_root")"
    protected_canonical_root="$(canonical_existing_dir "$protected_canonical_root")"
    protected_live_root="$(resolve_future_dir "$protected_live_root" "/")"
    PROTECTED_SOURCE_ROOT="$protected_source_root"
    PROTECTED_CANONICAL_ROOT="$protected_canonical_root"
    PROTECTED_LIVE_ROOT="$protected_live_root"

    observed_workspace_prefix="$(git -C "$PROJECT_ROOT" rev-parse --show-prefix)"
    observed_workspace_root="$REPOSITORY_ROOT"
    if [[ -n "$observed_workspace_prefix" ]]; then
        observed_workspace_root="$REPOSITORY_ROOT/${observed_workspace_prefix%/}"
    fi
    [[ "$PROJECT_ROOT" == "$observed_workspace_root" ]] || \
        fail "snapshot workspace path does not match its Git prefix"
    observed_commit="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{commit}')"
    observed_tree="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{tree}')"
    [[ -f "$PROJECT_ROOT/Cargo.lock" && ! -L "$PROJECT_ROOT/Cargo.lock" && \
        -s "$PROJECT_ROOT/Cargo.lock" ]] || fail "missing regular Cargo.lock"
    observed_lock="$(source_sha256 "$PROJECT_ROOT/Cargo.lock")"
    observed_status="$(git -C "$REPOSITORY_ROOT" status --porcelain=v1 --untracked-files=all)"
    [[ "$observed_commit" == "$IMPULSE_MACOS_BUILD_EXACT_COMMIT" ]] || \
        fail "detached source snapshot commit differs from coordinator capture"
    [[ "$observed_tree" == "$IMPULSE_MACOS_BUILD_EXACT_TREE" ]] || \
        fail "detached source snapshot tree differs from coordinator capture"
    [[ "$observed_lock" == "$IMPULSE_MACOS_BUILD_EXACT_LOCK_SHA256" ]] || \
        fail "detached source snapshot Cargo.lock differs from coordinator capture"
    [[ -z "$observed_status" ]] || fail "detached source snapshot is not clean"

    SOURCE_COMMIT="$observed_commit"
    SOURCE_TREE="$observed_tree"
    SOURCE_LOCK_DIGEST="$observed_lock"
    OUTPUT_ROOT="$(canonical_existing_dir "$output_root")"
    TARGET_DIR="$(canonical_existing_dir "$CARGO_TARGET_DIR")"
    require_target_child_path "$TARGET_DIR" "$OUTPUT_ROOT" "fresh Cargo build target"
    [[ -z "$(find "$TARGET_DIR" -mindepth 1 -print -quit)" ]] || \
        fail "fresh Cargo build target must be empty before Cargo starts"
    validate_target_dir "$OUTPUT_ROOT" \
        "detached source snapshot" "$REPOSITORY_ROOT" \
        "source worktree" "$PROTECTED_SOURCE_ROOT" \
        "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
        "live Impulse home" "$PROTECTED_LIVE_ROOT"
    validate_target_dir "$TARGET_DIR" \
        "detached source snapshot" "$REPOSITORY_ROOT" \
        "source worktree" "$PROTECTED_SOURCE_ROOT" \
        "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
        "live Impulse home" "$PROTECTED_LIVE_ROOT"
}

verify_source_unchanged() {
    local observed_commit observed_tree observed_lock observed_status
    observed_commit="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{commit}')"
    observed_tree="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{tree}')"
    observed_lock="$(source_sha256 "$PROJECT_ROOT/Cargo.lock")"
    observed_status="$(git -C "$REPOSITORY_ROOT" status --porcelain=v1 --untracked-files=all)"
    [[ "$observed_commit" == "$SOURCE_COMMIT" ]] || fail "source commit changed during packaging"
    [[ "$observed_tree" == "$SOURCE_TREE" ]] || fail "source tree changed during packaging"
    [[ "$observed_lock" == "$SOURCE_LOCK_DIGEST" ]] || \
        fail "Cargo.lock changed during packaging"
    [[ -z "$observed_status" ]] || fail "source worktree changed during packaging"
}

UNIVERSAL=false
CREATE_DMG=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --universal)
            UNIVERSAL=true
            shift
            ;;
        --dmg)
            CREATE_DMG=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown flag: $1"
            ;;
    esac
done

if [[ -z "${IMPULSE_MACOS_BUILD_EXACT_COMMIT:-}" ]]; then
    coordinate_exact_commit_build
    coordinator_status=$?
    exit "$coordinator_status"
fi
enter_exact_commit_build

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS app bundles must be built on macOS"
[[ -f "$DESKTOP_RESOURCES/Info.plist" && ! -L "$DESKTOP_RESOURCES/Info.plist" && \
    -s "$DESKTOP_RESOURCES/Info.plist" ]] || fail "missing Dioxus Info.plist template"
[[ -f "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" && \
    ! -L "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" && \
    -s "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" ]] || \
    fail "missing release-candidate notice"
[[ -f "$VERIFY_SCRIPT" && ! -L "$VERIFY_SCRIPT" && -s "$VERIFY_SCRIPT" ]] || \
    fail "missing bundle verifier"
[[ -f "$PROVENANCE_SCRIPT" && ! -L "$PROVENANCE_SCRIPT" && -s "$PROVENANCE_SCRIPT" ]] || \
    fail "missing source-provenance writer"
[[ -f "$LIVE_HOST_VERIFY_SCRIPT" && ! -L "$LIVE_HOST_VERIFY_SCRIPT" && \
    -s "$LIVE_HOST_VERIFY_SCRIPT" ]] || fail "missing packaged-host verifier"
[[ -d "$DESKTOP_CRATE/assets/vendor/xterm" && ! -L "$DESKTOP_CRATE/assets" && \
    ! -L "$DESKTOP_CRATE/assets/vendor" && ! -L "$DESKTOP_CRATE/assets/vendor/xterm" ]] || \
    fail "missing vendored Dioxus xterm assets"

RUNTIME_ASSETS=(
    "assets/impulse_crt.css"
    "assets/vendor/xterm/xterm.css"
    "assets/vendor/xterm/xterm.js"
    "assets/vendor/xterm/addon-fit.js"
    "assets/vendor/xterm/manifest.json"
    "assets/vendor/xterm/LICENSE.xterm.txt"
    "assets/vendor/xterm/LICENSE.addon-fit.txt"
)
for relative in "${RUNTIME_ASSETS[@]}"; do
    source_path="$DESKTOP_CRATE/$relative"
    [[ -f "$source_path" && ! -L "$source_path" && -s "$source_path" ]] || \
        fail "missing allowlisted runtime asset: $relative"
done

DESKTOP_VERSION="$(package_version "$DESKTOP_CRATE/Cargo.toml")"
CONTROL_VERSION="$(package_version "$PROJECT_ROOT/Cargo.toml")"
[[ -n "$DESKTOP_VERSION" && -n "$CONTROL_VERSION" ]] || \
    fail "could not read package versions"
[[ "$DESKTOP_VERSION" == "$CONTROL_VERSION" ]] || \
    fail "impulse-desktop and impulse-rs versions must match"
VERSION="$DESKTOP_VERSION"
[[ "$VERSION" =~ ^[0-9]+([.][0-9]+){2}$ ]] || \
    fail "package version must be semantic numeric x.y.z: $VERSION"

arch_suffix="$(uname -m)"
BUILD_TARGETS=()
if $UNIVERSAL; then
    arch_suffix="universal"
    BUILD_TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")
else
    case "$arch_suffix" in
        arm64) BUILD_TARGETS=("aarch64-apple-darwin") ;;
        x86_64) BUILD_TARGETS=("x86_64-apple-darwin") ;;
        *) fail "unsupported native macOS architecture: $arch_suffix" ;;
    esac
fi

PACKAGE_DIR="$(prepare_target_child_dir "$OUTPUT_ROOT/package" "$OUTPUT_ROOT" "package output" \
    "detached source snapshot" "$REPOSITORY_ROOT" \
    "source worktree" "$PROTECTED_SOURCE_ROOT" \
    "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
    "live Impulse home" "$PROTECTED_LIVE_ROOT")"
APP_DIR="$PACKAGE_DIR/$APP_NAME-$VERSION-macos-$arch_suffix-non-distributable-developer-preview.app"
ARCHIVE_ROOT="$(prepare_target_child_dir "$OUTPUT_ROOT/package-archives" "$OUTPUT_ROOT" \
    "package archive output" \
    "detached source snapshot" "$REPOSITORY_ROOT" \
    "source worktree" "$PROTECTED_SOURCE_ROOT" \
    "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
    "live Impulse home" "$PROTECTED_LIVE_ROOT")"
PACKAGE_STAGING_ROOT="$(prepare_target_child_dir "$OUTPUT_ROOT/package-staging" "$OUTPUT_ROOT" \
    "package staging output" \
    "detached source snapshot" "$REPOSITORY_ROOT" \
    "source worktree" "$PROTECTED_SOURCE_ROOT" \
    "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
    "live Impulse home" "$PROTECTED_LIVE_ROOT")"
PACKAGE_MOUNTS_ROOT="$(prepare_target_child_dir "$OUTPUT_ROOT/package-mounts" "$OUTPUT_ROOT" \
    "package mount output" \
    "detached source snapshot" "$REPOSITORY_ROOT" \
    "source worktree" "$PROTECTED_SOURCE_ROOT" \
    "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
    "live Impulse home" "$PROTECTED_LIVE_ROOT")"

recheck_packaging_output_roots() {
    local output_root
    for output_root in \
        "$OUTPUT_ROOT" \
        "$TARGET_DIR" \
        "$PACKAGE_DIR" \
        "$ARCHIVE_ROOT" \
        "$PACKAGE_STAGING_ROOT" \
        "$PACKAGE_MOUNTS_ROOT"; do
        recheck_target_dir "$output_root" \
            "detached source snapshot" "$REPOSITORY_ROOT" \
            "source worktree" "$PROTECTED_SOURCE_ROOT" \
            "canonical checkout" "$PROTECTED_CANONICAL_ROOT" \
            "live Impulse home" "$PROTECTED_LIVE_ROOT"
        if [[ "$output_root" != "$OUTPUT_ROOT" ]]; then
            require_target_child_path "$output_root" "$OUTPUT_ROOT" "packaging output"
        fi
    done
}

recheck_packaging_output_roots
echo "==> Building $APP_NAME v$VERSION (Dioxus Desktop + impulse-rs + ion)"
if $UNIVERSAL; then
    echo "==> Building arm64 and x86_64 release binaries"
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        (
            cd "$PROJECT_ROOT"
            cargo build --locked --release --target "$target" \
                -p impulse-desktop --features desktop-app --bin impulse-desktop
            cargo build --locked --release --target "$target" \
                -p impulse-rs --bin impulse-rs --bin ion
        )
    done
else
    echo "==> Building release binaries for $(uname -m)"
    (
        cd "$PROJECT_ROOT"
        cargo build --locked --release \
            -p impulse-desktop --features desktop-app --bin impulse-desktop
        cargo build --locked --release -p impulse-rs --bin impulse-rs --bin ion
    )
fi

recheck_packaging_output_roots
archive_existing "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

if $UNIVERSAL; then
    lipo -create \
        "$TARGET_DIR/aarch64-apple-darwin/release/impulse-desktop" \
        "$TARGET_DIR/x86_64-apple-darwin/release/impulse-desktop" \
        -output "$APP_DIR/Contents/MacOS/impulse-desktop"
    lipo -create \
        "$TARGET_DIR/aarch64-apple-darwin/release/impulse-rs" \
        "$TARGET_DIR/x86_64-apple-darwin/release/impulse-rs" \
        -output "$APP_DIR/Contents/MacOS/impulse-rs"
    lipo -create \
        "$TARGET_DIR/aarch64-apple-darwin/release/ion" \
        "$TARGET_DIR/x86_64-apple-darwin/release/ion" \
        -output "$APP_DIR/Contents/MacOS/ion"
else
    cp "$TARGET_DIR/release/impulse-desktop" "$APP_DIR/Contents/MacOS/impulse-desktop"
    cp "$TARGET_DIR/release/impulse-rs" "$APP_DIR/Contents/MacOS/impulse-rs"
    cp "$TARGET_DIR/release/ion" "$APP_DIR/Contents/MacOS/ion"
fi
chmod 0755 "$APP_DIR/Contents/MacOS/impulse-desktop" \
    "$APP_DIR/Contents/MacOS/impulse-rs" "$APP_DIR/Contents/MacOS/ion"

sed "s/__VERSION__/$VERSION/g" "$DESKTOP_RESOURCES/Info.plist" \
    > "$APP_DIR/Contents/Info.plist"
cp "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" \
    "$APP_DIR/Contents/Resources/ReleaseCandidateNotice.txt"
for relative in "${RUNTIME_ASSETS[@]}"; do
    destination="$APP_DIR/Contents/Resources/$relative"
    mkdir -p "$(dirname "$destination")"
    cp "$DESKTOP_CRATE/$relative" "$destination"
done
chmod 0644 "$APP_DIR/Contents/Info.plist" \
    "$APP_DIR/Contents/Resources/ReleaseCandidateNotice.txt"
for relative in "${RUNTIME_ASSETS[@]}"; do
    chmod 0644 "$APP_DIR/Contents/Resources/$relative"
done
chmod 0755 "$APP_DIR" \
    "$APP_DIR/Contents" \
    "$APP_DIR/Contents/MacOS" \
    "$APP_DIR/Contents/Resources" \
    "$APP_DIR/Contents/Resources/assets" \
    "$APP_DIR/Contents/Resources/assets/vendor" \
    "$APP_DIR/Contents/Resources/assets/vendor/xterm"

verify_source_unchanged
provenance_args=(--source-root "$PROJECT_ROOT" --version "$VERSION")
for target in "${BUILD_TARGETS[@]}"; do
    provenance_args+=(--target "$target")
done
bash "$PROVENANCE_SCRIPT" "${provenance_args[@]}" "$APP_DIR"
STAGED_PROVENANCE="$APP_DIR/Contents/Resources/$PROVENANCE_FILENAME"
[[ -f "$STAGED_PROVENANCE" ]] || \
    fail "source-provenance writer did not create $PROVENANCE_FILENAME"
STAGED_PROVENANCE_DIGEST="$(source_sha256 "$STAGED_PROVENANCE")"
echo "==> Embedded provenance SHA-256: sha256:$STAGED_PROVENANCE_DIGEST"
verify_source_unchanged

verify_args=(--macos --version "$VERSION" --source-root "$PROJECT_ROOT")
if $UNIVERSAL; then
    verify_args+=(--universal)
fi
bash "$VERIFY_SCRIPT" "${verify_args[@]}" "$APP_DIR"

if $CREATE_DMG; then
    recheck_packaging_output_roots
    dmg_name="$APP_NAME-$VERSION-macos-$arch_suffix-non-distributable-developer-preview.dmg"
    dmg_path="$PACKAGE_DIR/$dmg_name"
    dmg_stage="$(mktemp -d "$PACKAGE_STAGING_ROOT/stage.XXXXXX")"
    cp -R "$APP_DIR" "$dmg_stage/$APP_NAME.app"
    archive_existing "$dmg_path"

    echo "==> Creating $dmg_name"
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$dmg_stage" \
        -format UDZO \
        "$dmg_path" >/dev/null
    [[ -s "$dmg_path" ]] || fail "DMG creation did not produce a non-empty artifact"
    hdiutil verify "$dmg_path" >/dev/null || fail "DMG checksum verification failed"
    DMG_DIGEST="$(source_sha256 "$dmg_path")"

    mount_dir="$(mktemp -d "$PACKAGE_MOUNTS_ROOT/mount.XXXXXX")"
    DMG_MOUNT_DIR="$mount_dir"
    cleanup_dmg_mount() {
        if [[ -n "${DMG_MOUNT_DIR:-}" ]]; then
            hdiutil detach "$DMG_MOUNT_DIR" >/dev/null 2>&1 || true
        fi
    }
    trap cleanup_dmg_mount EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    hdiutil attach -nobrowse -readonly -mountpoint "$mount_dir" "$dmg_path" >/dev/null
    verify_mounted_volume_root "$mount_dir" "$APP_NAME.app"
    bash "$VERIFY_SCRIPT" "${verify_args[@]}" "$mount_dir/$APP_NAME.app"
    MOUNTED_PROVENANCE="$mount_dir/$APP_NAME.app/Contents/Resources/$PROVENANCE_FILENAME"
    cmp -s "$STAGED_PROVENANCE" "$MOUNTED_PROVENANCE" || \
        fail "mounted provenance manifest differs from the staged manifest"
    bash "$LIVE_HOST_VERIFY_SCRIPT" --source-root "$PROTECTED_SOURCE_ROOT" \
        "$mount_dir/$APP_NAME.app"
    verify_source_unchanged
    hdiutil detach "$mount_dir" >/dev/null || fail "failed to detach verified DMG"
    DMG_MOUNT_DIR=""
    trap - EXIT INT TERM
    hdiutil verify "$dmg_path" >/dev/null || fail "final DMG checksum verification failed"
    FINAL_DMG_DIGEST="$(source_sha256 "$dmg_path")"
    [[ "$FINAL_DMG_DIGEST" == "$DMG_DIGEST" ]] || \
        fail "DMG bytes changed during mounted verification"
    echo "==> DMG SHA-256: sha256:$FINAL_DMG_DIGEST"
    echo "==> DMG candidate created: $dmg_path"
fi

echo "==> Non-distributable Dioxus developer preview complete: $APP_DIR"
