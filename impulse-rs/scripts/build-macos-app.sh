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
APP_NAME="Impulse"

usage() {
    cat <<'EOF'
Usage: build-macos-app.sh [OPTIONS]

Build a macOS bundle containing the feature-gated Dioxus desktop host and the
impulse-rs control-plane companion plus native Ion sibling. Outputs are
non-distributable developer previews, and existing outputs are archived under
the Cargo target directory.

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
    local manifest="$1"
    awk -F '"' '/^version[[:space:]]*=[[:space:]]*"/ { print $2; exit }' "$manifest"
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

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS app bundles must be built on macOS"
[[ -f "$DESKTOP_RESOURCES/Info.plist" && ! -L "$DESKTOP_RESOURCES/Info.plist" && \
    -s "$DESKTOP_RESOURCES/Info.plist" ]] || fail "missing Dioxus Info.plist template"
[[ -f "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" && \
    ! -L "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" && \
    -s "$DESKTOP_RESOURCES/ReleaseCandidateNotice.txt" ]] || \
    fail "missing release-candidate notice"
[[ -f "$VERIFY_SCRIPT" && ! -L "$VERIFY_SCRIPT" && -s "$VERIFY_SCRIPT" ]] || \
    fail "missing bundle verifier"
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

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    if [[ "$CARGO_TARGET_DIR" = /* ]]; then
        TARGET_DIR="$CARGO_TARGET_DIR"
    else
        TARGET_DIR="$PROJECT_ROOT/$CARGO_TARGET_DIR"
    fi
else
    TARGET_DIR="$PROJECT_ROOT/target"
fi

arch_suffix="$(uname -m)"
if $UNIVERSAL; then
    arch_suffix="universal"
fi

PACKAGE_DIR="$TARGET_DIR/package"
APP_DIR="$PACKAGE_DIR/$APP_NAME-$VERSION-macos-$arch_suffix-non-distributable-developer-preview.app"
ARCHIVE_ROOT="$TARGET_DIR/package-archives"
ARCHIVE_SEQUENCE=0

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

mkdir -p "$PACKAGE_DIR"
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

verify_args=(--macos --version "$VERSION")
if $UNIVERSAL; then
    verify_args+=(--universal)
fi
bash "$VERIFY_SCRIPT" "${verify_args[@]}" "$APP_DIR"

if $CREATE_DMG; then
    dmg_name="$APP_NAME-$VERSION-macos-$arch_suffix-non-distributable-developer-preview.dmg"
    dmg_path="$PACKAGE_DIR/$dmg_name"
    stage_stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$"
    dmg_stage="$TARGET_DIR/package-staging/$stage_stamp"
    mkdir -p "$dmg_stage"
    cp -R "$APP_DIR" "$dmg_stage/$APP_NAME.app"
    archive_existing "$dmg_path"

    echo "==> Creating $dmg_name"
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$dmg_stage" \
        -format UDZO \
        "$dmg_path" >/dev/null
    [[ -s "$dmg_path" ]] || fail "DMG creation did not produce a non-empty artifact"
    hdiutil verify "$dmg_path" >/dev/null || fail "DMG checksum verification failed"

    mount_dir="$TARGET_DIR/package-mounts/$stage_stamp"
    mkdir -p "$mount_dir"
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
    bash "$VERIFY_SCRIPT" "${verify_args[@]}" "$mount_dir/$APP_NAME.app"
    hdiutil detach "$mount_dir" >/dev/null || fail "failed to detach verified DMG"
    DMG_MOUNT_DIR=""
    trap - EXIT INT TERM
    echo "==> DMG candidate created: $dmg_path"
fi

echo "==> Non-distributable Dioxus developer preview complete: $APP_DIR"
