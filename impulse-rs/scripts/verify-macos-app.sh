#!/usr/bin/env bash
# Verify the Dioxus-owned Impulse.app bundle. Structural checks are portable;
# Mach-O and universal-slice checks additionally require macOS.

set -euo pipefail

EXPECTED_VERSION=""
SOURCE_ROOT=""
CHECK_MACOS=false
CHECK_UNIVERSAL=false
APP_DIR=""
MANIFEST_NAME="ReleaseProvenance.v1.tsv"

usage() {
    cat <<'EOF'
Usage: verify-macos-app.sh [OPTIONS] APP_DIR

Options:
  --macos          Require valid Mach-O executables (requires macOS)
  --universal      Require arm64 and x86_64 slices (implies --macos)
  --version VER    Require the exact bundle version
  --source-root DIR
                   Bind the manifest to DIR's Git HEAD/tree and Cargo.lock
  --structure-only Run only portable bundle-layout checks (the default)
  -h, --help       Show this help
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

while [[ $# -gt 0 ]]; do
    case "$1" in
        --macos)
            CHECK_MACOS=true
            shift
            ;;
        --universal)
            CHECK_MACOS=true
            CHECK_UNIVERSAL=true
            shift
            ;;
        --version)
            require_value "$1" "${2:-}"
            EXPECTED_VERSION="$2"
            shift 2
            ;;
        --source-root)
            require_value "$1" "${2:-}"
            SOURCE_ROOT="$2"
            shift 2
            ;;
        --structure-only)
            CHECK_MACOS=false
            CHECK_UNIVERSAL=false
            shift
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

[[ -n "$APP_DIR" ]] || fail "APP_DIR is required"
[[ -n "$SOURCE_ROOT" ]] || fail "--source-root is required"

require_regular_file() {
    local path="$1"
    [[ -f "$path" && ! -L "$path" && -s "$path" ]] || \
        fail "required non-empty regular file is missing: $path"
}

require_executable() {
    local path="$1"
    require_regular_file "$path"
    [[ -x "$path" ]] || fail "required executable bit is missing: $path"
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
        *) printf '0%s' "$mode" ;;
    esac
}

plist_string() {
    local key="$1"
    awk -v key="$key" '
        index($0, "<key>" key "</key>") {
            while (getline) {
                if ($0 ~ /<string>.*<\/string>/) {
                    sub(/^.*<string>/, "")
                    sub(/<\/string>.*$/, "")
                    print
                    exit
                }
            }
        }
    ' "$PLIST"
}

[[ -d "$APP_DIR" && ! -L "$APP_DIR" ]] || fail "app bundle is missing: $APP_DIR"
[[ "$(portable_mode "$APP_DIR")" == "0755" ]] || \
    fail "app bundle root mode must be 0755"
CONTENTS="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
PLIST="$CONTENTS/Info.plist"
DESKTOP_BIN="$MACOS_DIR/impulse-desktop"
CONTROL_BIN="$MACOS_DIR/impulse-rs"
ION_BIN="$MACOS_DIR/ion"

require_regular_file "$PLIST"
require_executable "$DESKTOP_BIN"
require_executable "$CONTROL_BIN"
require_executable "$ION_BIN"
if [[ -e "$CONTENTS/_CodeSignature" || -L "$CONTENTS/_CodeSignature" ]]; then
    fail "developer preview must not contain a Developer ID bundle signature"
fi

[[ "$(plist_string CFBundleExecutable)" == "impulse-desktop" ]] || \
    fail "CFBundleExecutable must be impulse-desktop"
[[ "$(plist_string CFBundleIdentifier)" == "com.impulse.ai" ]] || \
    fail "CFBundleIdentifier must be com.impulse.ai"
[[ "$(plist_string CFBundlePackageType)" == "APPL" ]] || \
    fail "CFBundlePackageType must be APPL"

bundle_version="$(plist_string CFBundleVersion)"
bundle_short_version="$(plist_string CFBundleShortVersionString)"
[[ -n "$bundle_version" && -n "$bundle_short_version" ]] || \
    fail "bundle version metadata must not be blank"
[[ "$bundle_version" != *"__VERSION__"* && "$bundle_short_version" != *"__VERSION__"* ]] || \
    fail "Info.plist still contains an unstamped version placeholder"
[[ "$bundle_version" == "$bundle_short_version" ]] || \
    fail "bundle version fields do not agree"
if [[ -n "$EXPECTED_VERSION" ]]; then
    [[ "$bundle_version" == "$EXPECTED_VERSION" ]] || \
        fail "bundle version does not match $EXPECTED_VERSION"
fi

runtime_assets=(
    "ReleaseCandidateNotice.txt"
    "assets/impulse_crt.css"
    "assets/vendor/xterm/xterm.css"
    "assets/vendor/xterm/xterm.js"
    "assets/vendor/xterm/addon-fit.js"
    "assets/vendor/xterm/manifest.json"
    "assets/vendor/xterm/LICENSE.xterm.txt"
    "assets/vendor/xterm/LICENSE.addon-fit.txt"
)
for relative in "${runtime_assets[@]}"; do
    require_regular_file "$RESOURCES/$relative"
done

MANIFEST_RELATIVE="Contents/Resources/$MANIFEST_NAME"
MANIFEST="$APP_DIR/$MANIFEST_RELATIVE"
require_regular_file "$MANIFEST"
[[ "$(portable_mode "$MANIFEST")" == "0644" ]] || \
    fail "provenance manifest mode must be 0644"

[[ -d "$SOURCE_ROOT" && ! -L "$SOURCE_ROOT" ]] || \
    fail "source root must be a non-symlink directory: $SOURCE_ROOT"
SOURCE_ROOT="$(cd "$SOURCE_ROOT" && pwd -P)"
REPOSITORY_ROOT="$(git -C "$SOURCE_ROOT" rev-parse --show-toplevel 2>/dev/null)" || \
    fail "source root is not inside a Git worktree"
REPOSITORY_ROOT="$(cd "$REPOSITORY_ROOT" && pwd -P)"
WORKSPACE_PREFIX="$(git -C "$SOURCE_ROOT" rev-parse --show-prefix)"
WORKSPACE_LABEL="${WORKSPACE_PREFIX%/}"
if [[ -z "$WORKSPACE_LABEL" ]]; then
    WORKSPACE_LABEL="."
fi

OBSERVED_OBJECT_FORMAT="$(git -C "$REPOSITORY_ROOT" rev-parse --show-object-format)"
OBSERVED_COMMIT="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{commit}')"
OBSERVED_TREE="$(git -C "$REPOSITORY_ROOT" rev-parse --verify 'HEAD^{tree}')"
LOCK_FILE="$SOURCE_ROOT/Cargo.lock"
LOCK_RELATIVE="${WORKSPACE_PREFIX}Cargo.lock"
[[ -f "$LOCK_FILE" && ! -L "$LOCK_FILE" && -s "$LOCK_FILE" ]] || \
    fail "source workspace requires a non-empty regular Cargo.lock"
git -C "$REPOSITORY_ROOT" ls-files --error-unmatch -- "$LOCK_RELATIVE" >/dev/null 2>&1 || \
    fail "Cargo.lock must be tracked at $LOCK_RELATIVE"
git -C "$REPOSITORY_ROOT" diff --quiet HEAD -- "$LOCK_RELATIVE" || \
    fail "Cargo.lock differs from the source commit"
OBSERVED_LOCK_DIGEST="$(sha256_file "$LOCK_FILE")"
SOURCE_STATUS="$(git -C "$REPOSITORY_ROOT" status --porcelain=v1 --untracked-files=all)"
[[ -z "$SOURCE_STATUS" ]] || fail "source-bound verification requires a clean Git worktree"

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

MANIFEST_LINES=()
while IFS= read -r manifest_line || [[ -n "$manifest_line" ]]; do
    [[ "$manifest_line" != *$'\r'* ]] || fail "provenance manifest contains carriage returns"
    MANIFEST_LINES+=("$manifest_line")
done < "$MANIFEST"
MANIFEST_INDEX=0

expect_manifest_line() {
    local expected="$1"
    local label="$2"
    local observed="${MANIFEST_LINES[$MANIFEST_INDEX]-}"
    [[ "$observed" == "$expected" ]] || fail "provenance manifest $label mismatch"
    MANIFEST_INDEX=$((MANIFEST_INDEX + 1))
}

expect_manifest_line "IMPULSE_RELEASE_PROVENANCE_V1" "schema header"
expect_manifest_line $'source_object_format\t'"$OBSERVED_OBJECT_FORMAT" "source object format"
expect_manifest_line $'source_commit\t'"$OBSERVED_COMMIT" "source commit"
expect_manifest_line $'source_tree\t'"$OBSERVED_TREE" "source tree"
expect_manifest_line $'source_workspace\t'"$WORKSPACE_LABEL" "source workspace"
expect_manifest_line $'cargo_lock\t'"$LOCK_RELATIVE"$'\tsha256:'"$OBSERVED_LOCK_DIGEST" \
    "Cargo.lock digest"
expect_manifest_line $'bundle_version\t'"$bundle_version" "bundle version"
expect_manifest_line $'build_profile\trelease' "build profile"

MANIFEST_TARGETS=()
while [[ $MANIFEST_INDEX -lt ${#MANIFEST_LINES[@]} ]]; do
    target_line="${MANIFEST_LINES[$MANIFEST_INDEX]}"
    [[ "$target_line" == $'target\t'* ]] || break
    target="${target_line#$'target\t'}"
    [[ "$target" != *$'\t'* ]] || fail "provenance target record has extra fields"
    case "$target" in
        aarch64-apple-darwin|x86_64-apple-darwin) ;;
        *) fail "unsupported provenance target: $target" ;;
    esac
    MANIFEST_TARGETS+=("$target")
    MANIFEST_INDEX=$((MANIFEST_INDEX + 1))
done
case ${#MANIFEST_TARGETS[@]} in
    1)
        ;;
    2)
        [[ "${MANIFEST_TARGETS[0]}" == "aarch64-apple-darwin" && \
            "${MANIFEST_TARGETS[1]}" == "x86_64-apple-darwin" ]] || \
            fail "provenance targets must be unique and canonically ordered"
        ;;
    *) fail "provenance manifest must contain one or two targets" ;;
esac
expect_manifest_line $'inventory_exclusion\t'"$MANIFEST_RELATIVE"$'\tself' \
    "self-exclusion"

for expected_path in "${PAYLOAD_PATHS[@]}"; do
    record="${MANIFEST_LINES[$MANIFEST_INDEX]-}"
    record_kind=""
    recorded_path=""
    recorded_mode=""
    recorded_size=""
    recorded_digest=""
    extra=""
    IFS=$'\t' read -r record_kind recorded_path recorded_mode recorded_size recorded_digest extra \
        <<< "$record"
    [[ "$record_kind" == "file" && "$recorded_path" == "$expected_path" && -z "$extra" ]] || \
        fail "unexpected manifest record at payload $expected_path"
    case "$expected_path" in
        Contents/MacOS/*) expected_mode="0755" ;;
        *) expected_mode="0644" ;;
    esac
    [[ "$recorded_mode" == "$expected_mode" ]] || \
        fail "provenance mode mismatch for $expected_path"
    [[ "$recorded_size" =~ ^[0-9]+$ ]] || \
        fail "provenance size is invalid for $expected_path"
    [[ "$recorded_digest" == sha256:* ]] || \
        fail "provenance digest algorithm is invalid for $expected_path"
    digest_value="${recorded_digest#sha256:}"
    [[ ${#digest_value} -eq 64 && "$digest_value" != *[!0-9a-f]* ]] || \
        fail "provenance digest is invalid for $expected_path"

    payload="$APP_DIR/$expected_path"
    require_regular_file "$payload"
    actual_mode="$(portable_mode "$payload")"
    [[ "$actual_mode" == "$recorded_mode" ]] || \
        fail "payload mode differs from provenance for $expected_path"
    actual_size="$(wc -c < "$payload" | tr -d '[:space:]')"
    [[ "$actual_size" == "$recorded_size" ]] || \
        fail "payload size differs from provenance for $expected_path"
    actual_digest="$(sha256_file "$payload")"
    [[ "$actual_digest" == "$digest_value" ]] || \
        fail "payload digest differs from provenance for $expected_path"
    MANIFEST_INDEX=$((MANIFEST_INDEX + 1))
done
[[ $MANIFEST_INDEX -eq ${#MANIFEST_LINES[@]} ]] || \
    fail "unexpected manifest record after the closed payload inventory"

EXPECTED_DIRECTORIES=(
    "Contents"
    "Contents/MacOS"
    "Contents/Resources"
    "Contents/Resources/assets"
    "Contents/Resources/assets/vendor"
    "Contents/Resources/assets/vendor/xterm"
)

is_expected_bundle_file() {
    local candidate="$1"
    local expected
    [[ "$candidate" == "$MANIFEST_RELATIVE" ]] && return 0
    for expected in "${PAYLOAD_PATHS[@]}"; do
        [[ "$candidate" == "$expected" ]] && return 0
    done
    return 1
}

is_expected_bundle_directory() {
    local candidate="$1"
    local expected
    for expected in "${EXPECTED_DIRECTORIES[@]}"; do
        [[ "$candidate" == "$expected" ]] && return 0
    done
    return 1
}

if [[ -n "$(find "$APP_DIR" ! -type d ! -type f -print -quit)" ]]; then
    fail "app bundle must contain only regular files and allowlisted directories"
fi
while IFS= read -r -d '' bundle_file; do
    relative="${bundle_file#"$APP_DIR/"}"
    if ! is_expected_bundle_file "$relative"; then
        case "$relative" in
            Contents/Resources/*)
                fail "unexpected resource in app bundle: ${relative#Contents/Resources/}"
                ;;
            *) fail "unexpected bundle payload: $relative" ;;
        esac
    fi
done < <(find "$APP_DIR" -type f -print0)
while IFS= read -r -d '' bundle_directory; do
    [[ "$bundle_directory" == "$APP_DIR" ]] && continue
    relative="${bundle_directory#"$APP_DIR/"}"
    is_expected_bundle_directory "$relative" || \
        fail "unexpected bundle directory: $relative"
    [[ "$(portable_mode "$bundle_directory")" == "0755" ]] || \
        fail "bundle directory mode must be 0755: $relative"
done < <(find "$APP_DIR" -type d -print0)

if $CHECK_MACOS; then
    [[ "$(uname -s)" == "Darwin" ]] || fail "Mach-O verification requires macOS"
    plutil -lint "$PLIST" >/dev/null || fail "Info.plist is invalid"
    MANIFEST_REQUIRES_ARM64=false
    MANIFEST_REQUIRES_X86_64=false
    for target in "${MANIFEST_TARGETS[@]}"; do
        case "$target" in
            aarch64-apple-darwin) MANIFEST_REQUIRES_ARM64=true ;;
            x86_64-apple-darwin) MANIFEST_REQUIRES_X86_64=true ;;
        esac
    done
    for binary in "$DESKTOP_BIN" "$CONTROL_BIN" "$ION_BIN"; do
        file "$binary" | grep -F "Mach-O" >/dev/null || \
            fail "not a Mach-O executable: $binary"
        otool -L "$binary" >/dev/null || fail "invalid Mach-O load commands: $binary"
        archs="$(lipo -archs "$binary")"
        HAS_ARM64_SLICE=false
        HAS_X86_64_SLICE=false
        for arch in $archs; do
            case "$arch" in
                arm64) HAS_ARM64_SLICE=true ;;
                x86_64) HAS_X86_64_SLICE=true ;;
                *) fail "unexpected Mach-O architecture $arch: $binary" ;;
            esac
        done
        [[ "$HAS_ARM64_SLICE" == "$MANIFEST_REQUIRES_ARM64" ]] || \
            fail "arm64 slice does not match provenance targets: $binary"
        [[ "$HAS_X86_64_SLICE" == "$MANIFEST_REQUIRES_X86_64" ]] || \
            fail "x86_64 slice does not match provenance targets: $binary"
    done
    "$ION_BIN" --version >/dev/null || fail "packaged Ion binary did not launch"
fi

if $CHECK_UNIVERSAL; then
    for binary in "$DESKTOP_BIN" "$CONTROL_BIN" "$ION_BIN"; do
        archs="$(lipo -archs "$binary")"
        [[ " $archs " == *" arm64 "* ]] || fail "missing arm64 slice: $binary"
        [[ " $archs " == *" x86_64 "* ]] || fail "missing x86_64 slice: $binary"
    done
fi

echo "==> Verified source-bound non-distributable Dioxus bundle: $APP_DIR"
