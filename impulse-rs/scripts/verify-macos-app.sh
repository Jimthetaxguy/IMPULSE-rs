#!/usr/bin/env bash
# Verify the Dioxus-owned Impulse.app bundle. Structural checks are portable;
# Mach-O and universal-slice checks additionally require macOS.

set -euo pipefail

EXPECTED_VERSION=""
CHECK_MACOS=false
CHECK_UNIVERSAL=false
APP_DIR=""

usage() {
    cat <<'EOF'
Usage: verify-macos-app.sh [OPTIONS] APP_DIR

Options:
  --macos          Require valid Mach-O executables (requires macOS)
  --universal      Require arm64 and x86_64 slices (implies --macos)
  --version VER    Require the exact bundle version
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

is_expected_resource() {
    local candidate="$1"
    local expected
    for expected in "${runtime_assets[@]}"; do
        if [[ "$candidate" == "$expected" ]]; then
            return 0
        fi
    done
    return 1
}

if [[ -n "$(find "$RESOURCES" ! -type d ! -type f -print -quit)" ]]; then
    fail "resources must contain only regular files and allowlisted directories"
fi
while IFS= read -r resource_path; do
    relative="${resource_path#"$RESOURCES/"}"
    is_expected_resource "$relative" || fail "unexpected resource in app bundle: $relative"
done < <(find "$RESOURCES" -type f -print)
while IFS= read -r resource_dir; do
    [[ "$resource_dir" == "$RESOURCES" ]] && continue
    relative="${resource_dir#"$RESOURCES/"}"
    case "$relative" in
        assets|assets/vendor|assets/vendor/xterm) ;;
        *) fail "unexpected resource directory in app bundle: $relative" ;;
    esac
done < <(find "$RESOURCES" -type d -print)

if [[ -n "$(find "$CONTENTS" -type l -print -quit)" ]]; then
    fail "app bundle must not contain symlinked executables or resources"
fi

if $CHECK_MACOS; then
    [[ "$(uname -s)" == "Darwin" ]] || fail "Mach-O verification requires macOS"
    plutil -lint "$PLIST" >/dev/null || fail "Info.plist is invalid"
    for binary in "$DESKTOP_BIN" "$CONTROL_BIN" "$ION_BIN"; do
        file "$binary" | grep -F "Mach-O" >/dev/null || \
            fail "not a Mach-O executable: $binary"
        otool -L "$binary" >/dev/null || fail "invalid Mach-O load commands: $binary"
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

echo "==> Verified non-distributable Dioxus bundle: $APP_DIR"
