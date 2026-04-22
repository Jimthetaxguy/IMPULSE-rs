#!/usr/bin/env bash
# build-macos-app.sh — Build Impulse.app bundle and optional DMG.
#
# Usage:
#   bash scripts/build-macos-app.sh               # Build .app for current arch
#   bash scripts/build-macos-app.sh --universal    # Universal binary (arm64+x86_64)
#   bash scripts/build-macos-app.sh --dmg          # Also create DMG
#   bash scripts/build-macos-app.sh --universal --dmg --sign "Developer ID Application: ..."
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GUI_RESOURCES="$PROJECT_ROOT/impulse-gui/resources"
APP_NAME="Impulse"
BUNDLE_ID="com.impulse.ai"

# Parse version from impulse-gui/Cargo.toml
VERSION=$(grep '^version' "$PROJECT_ROOT/impulse-gui/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

# Defaults
UNIVERSAL=false
CREATE_DMG=false
SIGN_IDENTITY=""
RELEASE_DIR="$PROJECT_ROOT/target/release"

# Parse flags
while [[ $# -gt 0 ]]; do
    case "$1" in
        --universal) UNIVERSAL=true; shift ;;
        --dmg)       CREATE_DMG=true; shift ;;
        --sign)      SIGN_IDENTITY="$2"; shift 2 ;;
        --version)   VERSION="$2"; shift 2 ;;
        *) echo "Unknown flag: $1"; exit 1 ;;
    esac
done

echo "==> Building $APP_NAME v$VERSION"

# ---------------------------------------------------------------------------
# Step 1: Build binaries
# ---------------------------------------------------------------------------
if $UNIVERSAL; then
    echo "==> Building universal binaries (aarch64 + x86_64)"
    (cd "$PROJECT_ROOT" && cargo build --release --target aarch64-apple-darwin)
    (cd "$PROJECT_ROOT" && cargo build --release --target x86_64-apple-darwin)
    ARM_DIR="$PROJECT_ROOT/target/aarch64-apple-darwin/release"
    X86_DIR="$PROJECT_ROOT/target/x86_64-apple-darwin/release"
else
    echo "==> Building for current architecture"
    (cd "$PROJECT_ROOT" && cargo build --release)
fi

# ---------------------------------------------------------------------------
# Step 2: Assemble .app bundle
# ---------------------------------------------------------------------------
APP_DIR="$PROJECT_ROOT/$APP_NAME.app"
rm -rf "$APP_DIR"

mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

echo "==> Assembling $APP_NAME.app"

# Copy binaries
if $UNIVERSAL; then
    lipo -create \
        "$ARM_DIR/impulse-gui" "$X86_DIR/impulse-gui" \
        -output "$APP_DIR/Contents/MacOS/impulse-gui"
    lipo -create \
        "$ARM_DIR/impulse-rs" "$X86_DIR/impulse-rs" \
        -output "$APP_DIR/Contents/MacOS/impulse-rs"
else
    cp "$RELEASE_DIR/impulse-gui" "$APP_DIR/Contents/MacOS/impulse-gui"
    cp "$RELEASE_DIR/impulse-rs"  "$APP_DIR/Contents/MacOS/impulse-rs"
fi

chmod +x "$APP_DIR/Contents/MacOS/impulse-gui"
chmod +x "$APP_DIR/Contents/MacOS/impulse-rs"

# Copy Info.plist and stamp version
sed "s/__VERSION__/$VERSION/g" "$GUI_RESOURCES/Info.plist" \
    > "$APP_DIR/Contents/Info.plist"

# Copy icon if it exists
if [[ -f "$GUI_RESOURCES/Impulse.icns" ]]; then
    cp "$GUI_RESOURCES/Impulse.icns" "$APP_DIR/Contents/Resources/Impulse.icns"
else
    echo "  (!) No Impulse.icns found — app will use default icon"
fi

echo "==> $APP_NAME.app created at $APP_DIR"

# ---------------------------------------------------------------------------
# Step 3: Code signing (optional)
# ---------------------------------------------------------------------------
if [[ -n "$SIGN_IDENTITY" ]]; then
    echo "==> Signing with: $SIGN_IDENTITY"
    codesign --deep --force --options runtime \
        --sign "$SIGN_IDENTITY" \
        "$APP_DIR"
    echo "==> Verifying signature"
    codesign --verify --deep --strict "$APP_DIR"
fi

# ---------------------------------------------------------------------------
# Step 4: Create DMG (optional)
# ---------------------------------------------------------------------------
if $CREATE_DMG; then
    ARCH_SUFFIX="universal"
    if ! $UNIVERSAL; then
        ARCH_SUFFIX="$(uname -m)"
    fi
    DMG_NAME="$APP_NAME-${VERSION}-macos-${ARCH_SUFFIX}.dmg"
    DMG_PATH="$PROJECT_ROOT/$DMG_NAME"

    echo "==> Creating DMG: $DMG_NAME"

    # Use create-dmg if available (prettier), otherwise hdiutil
    if command -v create-dmg &>/dev/null; then
        # create-dmg fails if the dmg already exists
        rm -f "$DMG_PATH"
        create-dmg \
            --volname "$APP_NAME" \
            --window-pos 200 120 \
            --window-size 600 400 \
            --icon-size 100 \
            --icon "$APP_NAME.app" 175 190 \
            --app-drop-link 425 190 \
            --hide-extension "$APP_NAME.app" \
            "$DMG_PATH" \
            "$APP_DIR"
    else
        hdiutil create -volname "$APP_NAME" \
            -srcfolder "$APP_DIR" \
            -ov -format UDZO \
            "$DMG_PATH"
    fi

    echo "==> DMG created: $DMG_PATH"
fi

echo "==> Done!"
