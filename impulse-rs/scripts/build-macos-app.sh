#!/usr/bin/env bash
# Build the real Dioxus Impulse.app bundle and, optionally, a DMG.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DESKTOP_CRATE="$PROJECT_ROOT/impulse-desktop"
DESKTOP_RESOURCES="$DESKTOP_CRATE/resources"
VERIFY_SCRIPT="$SCRIPT_DIR/verify-macos-app.sh"
APP_NAME="Impulse"

usage() {
    cat <<'EOF'
Usage: build-macos-app.sh [OPTIONS]

Build a macOS bundle containing the Dioxus desktop host and its impulse-rs
control-plane companion. Existing app/DMG outputs are moved into target/package-archives.

Options:
  --universal          Build arm64 + x86_64 Mach-O binaries with lipo
  --dmg                Create a DMG after bundle verification
  --smoke              Launch the packaged app and require its live-host receipt
  --smoke-timeout SEC  Bound the packaged launch smoke (default: 45)
  --sign IDENTITY      Sign with IDENTITY instead of the default ad-hoc identity
  --version VERSION    Override the impulse-desktop package version
  -h, --help           Show this help
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

archive_existing() {
    local path="$1"
    if [[ ! -e "$path" && ! -L "$path" ]]; then
        return
    fi

    ARCHIVE_SEQUENCE=$((ARCHIVE_SEQUENCE + 1))
    local stamp
    stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$-$ARCHIVE_SEQUENCE"
    local archive_dir="$ARCHIVE_ROOT/$stamp"
    mkdir -p "$archive_dir"
    mv "$path" "$archive_dir/$(basename "$path")"
    echo "==> Archived existing $(basename "$path") to $archive_dir"
}

package_version() {
    awk -F '"' '/^version[[:space:]]*=[[:space:]]*"/ { print $2; exit }' \
        "$DESKTOP_CRATE/Cargo.toml"
}

UNIVERSAL=false
CREATE_DMG=false
RUN_SMOKE=false
SIGN_IDENTITY="-"
SMOKE_TIMEOUT=45
VERSION="$(package_version)"

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
        --smoke)
            RUN_SMOKE=true
            shift
            ;;
        --smoke-timeout)
            require_value "$1" "${2:-}"
            SMOKE_TIMEOUT="$2"
            shift 2
            ;;
        --sign)
            require_value "$1" "${2:-}"
            SIGN_IDENTITY="$2"
            shift 2
            ;;
        --version)
            require_value "$1" "${2:-}"
            VERSION="$2"
            shift 2
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

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS app bundles must be built on macOS"
[[ "$VERSION" =~ ^[0-9]+([.][0-9]+){0,3}$ ]] || \
    fail "bundle version must contain one to four numeric components: $VERSION"
[[ "$SMOKE_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || fail "smoke timeout must be a positive integer"
[[ -x "$VERIFY_SCRIPT" ]] || fail "missing executable verifier: $VERIFY_SCRIPT"
[[ -f "$DESKTOP_RESOURCES/Info.plist" ]] || fail "missing Dioxus Info.plist template"
[[ -f "$DESKTOP_RESOURCES/Impulse.icns" ]] || fail "missing Dioxus icon"
[[ -d "$DESKTOP_CRATE/assets/vendor/xterm" ]] || fail "missing vendored Dioxus xterm assets"

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    if [[ "$CARGO_TARGET_DIR" = /* ]]; then
        TARGET_DIR="$CARGO_TARGET_DIR"
    else
        TARGET_DIR="$PROJECT_ROOT/$CARGO_TARGET_DIR"
    fi
else
    TARGET_DIR="$PROJECT_ROOT/target"
fi

APP_DIR="$PROJECT_ROOT/$APP_NAME.app"
ARCHIVE_ROOT="$TARGET_DIR/package-archives"
ARCHIVE_SEQUENCE=0

echo "==> Building $APP_NAME v$VERSION (Dioxus Desktop + impulse-rs)"

if $UNIVERSAL; then
    echo "==> Building aarch64 and x86_64 release binaries"
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        (
            cd "$PROJECT_ROOT"
            cargo build --locked --release --target "$target" \
                -p impulse-desktop --features desktop-app --bin impulse-desktop
            cargo build --locked --release --target "$target" \
                -p impulse-rs --bin impulse-rs
        )
    done
else
    echo "==> Building release binaries for the current architecture"
    (
        cd "$PROJECT_ROOT"
        cargo build --locked --release \
            -p impulse-desktop --features desktop-app --bin impulse-desktop
        cargo build --locked --release -p impulse-rs --bin impulse-rs
    )
fi

archive_existing "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

echo "==> Assembling $APP_DIR"
if $UNIVERSAL; then
    lipo -create \
        "$TARGET_DIR/aarch64-apple-darwin/release/impulse-desktop" \
        "$TARGET_DIR/x86_64-apple-darwin/release/impulse-desktop" \
        -output "$APP_DIR/Contents/MacOS/impulse-desktop"
    lipo -create \
        "$TARGET_DIR/aarch64-apple-darwin/release/impulse-rs" \
        "$TARGET_DIR/x86_64-apple-darwin/release/impulse-rs" \
        -output "$APP_DIR/Contents/MacOS/impulse-rs"
else
    cp "$TARGET_DIR/release/impulse-desktop" "$APP_DIR/Contents/MacOS/impulse-desktop"
    cp "$TARGET_DIR/release/impulse-rs" "$APP_DIR/Contents/MacOS/impulse-rs"
fi
chmod 0755 "$APP_DIR/Contents/MacOS/impulse-desktop" "$APP_DIR/Contents/MacOS/impulse-rs"

sed "s/__VERSION__/$VERSION/g" "$DESKTOP_RESOURCES/Info.plist" \
    > "$APP_DIR/Contents/Info.plist"
cp "$DESKTOP_RESOURCES/Impulse.icns" "$APP_DIR/Contents/Resources/Impulse.icns"
cp -R "$DESKTOP_CRATE/assets" "$APP_DIR/Contents/Resources/assets"

if [[ "$SIGN_IDENTITY" == "-" ]]; then
    echo "==> Ad-hoc signing post-build binaries and bundle"
else
    echo "==> Signing $APP_NAME.app with: $SIGN_IDENTITY"
fi
sign_args=(--force --options runtime --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" != "-" ]]; then
    sign_args+=(--timestamp)
fi
codesign "${sign_args[@]}" "$APP_DIR/Contents/MacOS/impulse-rs"
codesign "${sign_args[@]}" "$APP_DIR/Contents/MacOS/impulse-desktop"
codesign "${sign_args[@]}" "$APP_DIR"

bundle_verify_args=(--macos --signed --version "$VERSION")
if $UNIVERSAL; then
    bundle_verify_args+=(--universal)
fi
verify_args=("${bundle_verify_args[@]}")
if $RUN_SMOKE; then
    verify_args+=(--smoke --timeout "$SMOKE_TIMEOUT")
fi
"$VERIFY_SCRIPT" "${verify_args[@]}" "$APP_DIR"

if $CREATE_DMG; then
    arch_suffix="$(uname -m)"
    if $UNIVERSAL; then
        arch_suffix="universal"
    fi
    # Developer ID signing can be supplied, but notarization is deliberately
    # not implied by this script. Keep the published filename honest until a
    # notarization/stapling contract is wired into the release workflow.
    dmg_name="$APP_NAME-${VERSION}-macos-${arch_suffix}-developer-preview.dmg"
    dmg_dir="$TARGET_DIR/package"
    dmg_path="$dmg_dir/$dmg_name"
    stage_stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$"
    dmg_stage="$TARGET_DIR/package-staging/$stage_stamp"
    mkdir -p "$dmg_dir"
    archive_existing "$dmg_path"
    mkdir -p "$dmg_stage"
    cp -R "$APP_DIR" "$dmg_stage/$APP_NAME.app"

    echo "==> Creating $dmg_name"
    if command -v create-dmg >/dev/null 2>&1; then
        create-dmg \
            --volname "$APP_NAME" \
            --window-pos 200 120 \
            --window-size 600 400 \
            --icon-size 100 \
            --icon "$APP_NAME.app" 175 190 \
            --app-drop-link 425 190 \
            --hide-extension "$APP_NAME.app" \
            "$dmg_path" \
            "$dmg_stage"
    else
        hdiutil create -volname "$APP_NAME" \
            -srcfolder "$dmg_stage" \
            -format UDZO \
            "$dmg_path"
    fi
    [[ -s "$dmg_path" ]] || fail "DMG creation did not produce a non-empty artifact"
    hdiutil verify "$dmg_path" >/dev/null || fail "DMG checksum verification failed"

    mount_root="$TARGET_DIR/package-mounts"
    mount_dir="$mount_root/$stage_stamp"
    mount_log="$mount_root/$stage_stamp.attach.log"
    mkdir -p "$mount_dir"
    DMG_MOUNT_DIR="$mount_dir"
    cleanup_dmg_mount() {
        if [[ -n "$DMG_MOUNT_DIR" ]]; then
            hdiutil detach "$DMG_MOUNT_DIR" >/dev/null 2>&1 || true
        fi
    }
    trap cleanup_dmg_mount EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    hdiutil attach -nobrowse -readonly -mountpoint "$mount_dir" "$dmg_path" >"$mount_log"
    "$VERIFY_SCRIPT" "${bundle_verify_args[@]}" "$mount_dir/$APP_NAME.app"
    hdiutil detach "$mount_dir" >/dev/null || fail "failed to detach verified DMG"
    DMG_MOUNT_DIR=""
    trap - EXIT INT TERM
    echo "==> DMG created: $dmg_path"
fi

echo "==> Dioxus macOS package complete"
