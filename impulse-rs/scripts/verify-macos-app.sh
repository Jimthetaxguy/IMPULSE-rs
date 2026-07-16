#!/usr/bin/env bash
# Verify the Dioxus Impulse.app bundle. Structural checks run cross-platform;
# Mach-O, signature, universal-architecture, and live smoke checks require macOS.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/Impulse.app"
EXPECTED_VERSION=""
CHECK_MACOS=false
CHECK_UNIVERSAL=false
CHECK_SIGNED=false
RUN_SMOKE=false
SMOKE_TIMEOUT=45

usage() {
    cat <<'EOF'
Usage: verify-macos-app.sh [OPTIONS] [APP_DIR]

Options:
  --macos          Require real Mach-O executables (requires macOS)
  --universal      Require both arm64 and x86_64 slices (implies --macos)
  --signed         Require a valid strict code signature (implies --macos)
  --smoke          Launch the bundle and require IMPULSE_DESKTOP_SMOKE_RECEIPT
  --timeout SEC    Bound the launch smoke (default: 45)
  --version VER    Require the exact bundle version
  --structure-only Skip platform-specific binary checks
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
        --signed)
            CHECK_MACOS=true
            CHECK_SIGNED=true
            shift
            ;;
        --smoke)
            CHECK_MACOS=true
            RUN_SMOKE=true
            shift
            ;;
        --timeout)
            require_value "$1" "${2:-}"
            SMOKE_TIMEOUT="$2"
            shift 2
            ;;
        --version)
            require_value "$1" "${2:-}"
            EXPECTED_VERSION="$2"
            shift 2
            ;;
        --structure-only)
            CHECK_MACOS=false
            CHECK_UNIVERSAL=false
            CHECK_SIGNED=false
            RUN_SMOKE=false
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
            [[ "$APP_DIR" == "$PROJECT_ROOT/Impulse.app" ]] || \
                fail "only one APP_DIR may be supplied"
            APP_DIR="$1"
            shift
            ;;
    esac
done

[[ "$SMOKE_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || fail "timeout must be a positive integer"

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
ICON="$RESOURCES/Impulse.icns"

require_regular_file "$PLIST"
require_executable "$DESKTOP_BIN"
require_executable "$CONTROL_BIN"
require_regular_file "$ICON"

[[ "$(plist_string CFBundleExecutable)" == "impulse-desktop" ]] || \
    fail "CFBundleExecutable must be impulse-desktop"
[[ "$(plist_string CFBundleIdentifier)" == "com.impulse.ai" ]] || \
    fail "unexpected CFBundleIdentifier"
[[ "$(plist_string CFBundlePackageType)" == "APPL" ]] || \
    fail "CFBundlePackageType must be APPL"
[[ "$(plist_string CFBundleIconFile)" == "Impulse.icns" ]] || \
    fail "CFBundleIconFile must be Impulse.icns"
bundle_version="$(plist_string CFBundleVersion)"
bundle_short_version="$(plist_string CFBundleShortVersionString)"
[[ -n "$bundle_version" && -n "$bundle_short_version" ]] || \
    fail "bundle version metadata must not be blank"
[[ "$bundle_version" != *"__VERSION__"* ]] || \
    fail "Info.plist still contains an unstamped version placeholder"
if [[ -n "$EXPECTED_VERSION" ]]; then
    [[ "$bundle_version" == "$EXPECTED_VERSION" ]] || \
        fail "CFBundleVersion does not match $EXPECTED_VERSION"
    [[ "$bundle_short_version" == "$EXPECTED_VERSION" ]] || \
        fail "CFBundleShortVersionString does not match $EXPECTED_VERSION"
fi

[[ "$(LC_ALL=C dd if="$ICON" bs=1 count=4 2>/dev/null)" == "icns" ]] || \
    fail "Impulse.icns does not have an icns header"

runtime_assets=(
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
if [[ -n "$(find "$CONTENTS" -type l -print -quit)" ]]; then
    fail "app bundle must not contain symlinked executables or resources"
fi

if $CHECK_MACOS; then
    [[ "$(uname -s)" == "Darwin" ]] || fail "Mach-O verification requires macOS"
    for binary in "$DESKTOP_BIN" "$CONTROL_BIN"; do
        file "$binary" | grep -F "Mach-O" >/dev/null || fail "not a Mach-O executable: $binary"
        otool -L "$binary" >/dev/null || fail "Mach-O load commands are invalid: $binary"
    done
fi

if $CHECK_UNIVERSAL; then
    for binary in "$DESKTOP_BIN" "$CONTROL_BIN"; do
        archs="$(lipo -archs "$binary")"
        [[ " $archs " == *" arm64 "* ]] || fail "missing arm64 slice: $binary"
        [[ " $archs " == *" x86_64 "* ]] || fail "missing x86_64 slice: $binary"
    done
fi

if $CHECK_SIGNED; then
    codesign --verify --deep --strict "$APP_DIR" || fail "bundle signature verification failed"
fi

if $CHECK_MACOS; then
    scope_stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$"
    scope_dir="$PROJECT_ROOT/target/package-scope-probe/$scope_stamp"
    # Finder-style launch proof must run outside the repository tree. Keeping
    # the probe cwd below PROJECT_ROOT would let ancestor discovery find the
    # repository's own `.impulse` directory and produce a false failure.
    scope_workspace="${TMPDIR:-/tmp}/impulse-package-scope-workspace/$scope_stamp"
    scope_home="$scope_dir/home"
    scope_log="$scope_dir/desktop-scope.log"
    mkdir -p "$scope_workspace" "$scope_home"
    (
        cd "$scope_workspace"
        exec env \
            -u IMPULSE_HOME \
            -u IMPULSE_SOCKET_PATH \
            HOME="$scope_home" \
            IMPULSE_DESKTOP_SCOPE_PROBE=1 \
            "$DESKTOP_BIN"
    ) >"$scope_log" 2>&1 || fail "packaged desktop scope probe failed; see $scope_log"
    scope_prefix="IMPULSE_DESKTOP_SCOPE_RECEIPT "
    scope_line="$(grep -F "$scope_prefix" "$scope_log" | tail -n 1)"
    [[ -n "$scope_line" ]] || fail "packaged desktop did not emit its scope receipt; see $scope_log"
    scope_json="${scope_line#*"$scope_prefix"}"
    scope_value() {
        printf '%s' "$scope_json" | plutil -extract "$1" raw -o - - 2>/dev/null
    }
    [[ "$(scope_value status)" == "desktop-scope-resolved" ]] || \
        fail "packaged desktop emitted an unexpected scope status; see $scope_log"
    [[ "$(scope_value daemon_configured)" == "false" ]] || \
        fail "no-env packaged desktop promoted an implicit daemon boundary; see $scope_log"
    [[ "$scope_json" == *'"project_root":null'* ]] || \
        fail "no-env packaged desktop claimed an implicit project root; see $scope_log"
    [[ "$scope_json" == *'"memory_root":null'* ]] || \
        fail "no-env packaged desktop claimed an implicit project memory root; see $scope_log"
    [[ ! -e "$scope_home/.impulse" && ! -e "$scope_workspace/.impulse" ]] || \
        fail "scope resolution created project or memory state before user selection; see $scope_log"
    echo "==> Packaged default scope probe passed; evidence retained at $scope_dir"
fi

SMOKE_DESKTOP_PID=""
SMOKE_DAEMON_PID=""
SMOKE_WORKER_PID=""
SMOKE_WORKER_PID_PATH=""
SMOKE_RUNTIME_DIR=""
SMOKE_EVIDENCE_DIR=""
cleanup_smoke_processes() {
    if [[ -z "$SMOKE_WORKER_PID" && -n "$SMOKE_WORKER_PID_PATH" && -s "$SMOKE_WORKER_PID_PATH" ]]; then
        candidate_worker_pid="$(tr -d '[:space:]' <"$SMOKE_WORKER_PID_PATH")"
        if [[ "$candidate_worker_pid" =~ ^[1-9][0-9]*$ ]]; then
            SMOKE_WORKER_PID="$candidate_worker_pid"
        fi
    fi
    for pid in "$SMOKE_DESKTOP_PID" "$SMOKE_DAEMON_PID" "$SMOKE_WORKER_PID"; do
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done
    for pid in "$SMOKE_DESKTOP_PID" "$SMOKE_DAEMON_PID" "$SMOKE_WORKER_PID"; do
        if [[ -z "$pid" ]]; then
            continue
        fi
        for _ in {1..40}; do
            if ! kill -0 "$pid" 2>/dev/null; then
                break
            fi
            sleep 0.05
        done
        if kill -0 "$pid" 2>/dev/null; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
        wait "$pid" 2>/dev/null || true
    done
    if [[ -n "$SMOKE_RUNTIME_DIR" && -d "$SMOKE_RUNTIME_DIR" && -n "$SMOKE_EVIDENCE_DIR" ]]; then
        mkdir -p "$SMOKE_EVIDENCE_DIR"
        mv "$SMOKE_RUNTIME_DIR" "$SMOKE_EVIDENCE_DIR/runtime"
    fi
    return 0
}

process_is_running() {
    local pid="$1"
    if ! kill -0 "$pid" 2>/dev/null; then
        return 1
    fi
    local state
    state="$(ps -o stat= -p "$pid" 2>/dev/null | tr -d '[:space:]')"
    [[ -n "$state" && "$state" != Z* ]]
}

if $RUN_SMOKE; then
    smoke_stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$"
    smoke_dir="$PROJECT_ROOT/target/package-smoke/$smoke_stamp"
    # AF_UNIX paths are short on macOS (SUN_LEN). Keep the live socket under
    # /tmp, then move the complete runtime tree into retained evidence after
    # both exact child PIDs have been reaped.
    smoke_runtime="/tmp/impulse-smoke-$$-$RANDOM"
    smoke_workspace="$smoke_runtime/workspace"
    smoke_home="$smoke_runtime/home"
    impulse_dir="$smoke_workspace/.impulse"
    socket_path="$impulse_dir/sockets/impulse.sock"
    daemon_pid_path="${socket_path%.sock}.pid"
    shutdown_worker_pid_path="$impulse_dir/desktop-shutdown-worker.pid"
    SMOKE_WORKER_PID_PATH="$shutdown_worker_pid_path"
    desktop_log="$smoke_dir/desktop.log"
    [[ ! -e "$smoke_runtime" && ! -L "$smoke_runtime" ]] || \
        fail "short smoke runtime path already exists: $smoke_runtime"
    mkdir -p "$smoke_workspace" "$smoke_home" "$smoke_dir"
    SMOKE_RUNTIME_DIR="$smoke_runtime"
    SMOKE_EVIDENCE_DIR="$smoke_dir"
    trap cleanup_smoke_processes EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    (
        cd "$smoke_workspace"
        HOME="$smoke_home" "$CONTROL_BIN" -c "$impulse_dir" init
    ) >"$smoke_dir/init.log" 2>&1 || fail "packaged impulse-rs init failed"

    (
        cd "$smoke_workspace"
        exec env \
            HOME="$smoke_home" \
            IMPULSE_HOME="$impulse_dir" \
            IMPULSE_SOCKET_PATH="$socket_path" \
            IMPULSE_CONTROL_CLI="$CONTROL_BIN" \
            IMPULSE_DESKTOP_SMOKE=1 \
            "$DESKTOP_BIN"
    ) >"$desktop_log" 2>&1 &
    SMOKE_DESKTOP_PID=$!

    sidecar_deadline=$((SECONDS + 20))
    while [[ ! -S "$socket_path" || ! -s "$daemon_pid_path" ]]; do
        kill -0 "$SMOKE_DESKTOP_PID" 2>/dev/null || \
            fail "packaged desktop exited before starting its daemon sidecar; see $desktop_log"
        (( SECONDS < sidecar_deadline )) || \
            fail "packaged desktop did not start its daemon sidecar; see $desktop_log"
        sleep 0.1
    done
    SMOKE_DAEMON_PID="$(tr -d '[:space:]' <"$daemon_pid_path")"
    [[ "$SMOKE_DAEMON_PID" =~ ^[1-9][0-9]*$ ]] || \
        fail "packaged daemon PID file is malformed: $daemon_pid_path"
    kill -0 "$SMOKE_DAEMON_PID" 2>/dev/null || \
        fail "packaged daemon sidecar PID is not alive: $SMOKE_DAEMON_PID"
    grep -F "desktop started packaged Impulse daemon companion" "$desktop_log" >/dev/null || \
        fail "desktop did not report spawned daemon ownership; see $desktop_log"

    receipt_prefix="IMPULSE_DESKTOP_SMOKE_RECEIPT "
    smoke_deadline=$((SECONDS + SMOKE_TIMEOUT))
    while ! grep -F "$receipt_prefix" "$desktop_log" >/dev/null 2>&1; do
        kill -0 "$SMOKE_DESKTOP_PID" 2>/dev/null || \
            fail "packaged desktop exited before its smoke receipt; see $desktop_log"
        (( SECONDS < smoke_deadline )) || \
            fail "packaged desktop smoke timed out; see $desktop_log"
        sleep 0.1
    done

    receipt_line="$(grep -F "$receipt_prefix" "$desktop_log" | tail -n 1)"
    receipt_json="${receipt_line#*"$receipt_prefix"}"
    receipt_value() {
        printf '%s' "$receipt_json" | plutil -extract "$1" raw -o - - 2>/dev/null
    }
    if ! receipt_status="$(receipt_value status)"; then
        fail "package smoke emitted malformed receipt JSON; see $desktop_log"
    fi
    [[ "$receipt_status" == "dioxus-eval-bridge-ready" ]] || \
        fail "unexpected package smoke status: $receipt_status"
    receipt_session_id="$(receipt_value session_id)" || \
        fail "package smoke receipt is missing session_id"
    [[ -n "${receipt_session_id//[[:space:]]/}" ]] || \
        fail "package smoke receipt has a blank session_id"
    for key in \
        bridge_ready \
        assets_ready \
        terminal_opened \
        terminal_output_seen \
        terminal_resized \
        terminal_focused \
        terminal_closed \
        terminal_exit_seen \
        ops_update_seen; do
        value="$(receipt_value "$key")" || \
            fail "package smoke receipt is missing $key"
        [[ "$value" == "true" ]] || fail "package smoke receipt did not prove $key"
    done

    desktop_exit_deadline=$((SECONDS + 15))
    while process_is_running "$SMOKE_DESKTOP_PID"; do
        (( SECONDS < desktop_exit_deadline )) || \
            fail "packaged desktop did not exit after its ordered smoke close; see $desktop_log"
        sleep 0.1
    done
    if ! wait "$SMOKE_DESKTOP_PID"; then
        fail "packaged desktop returned a failure status after its smoke receipt; see $desktop_log"
    fi
    SMOKE_DESKTOP_PID=""
    [[ -s "$shutdown_worker_pid_path" ]] || \
        fail "packaged shutdown probe did not record its exact worker PID; see $desktop_log"
    SMOKE_WORKER_PID="$(tr -d '[:space:]' <"$shutdown_worker_pid_path")"
    [[ "$SMOKE_WORKER_PID" =~ ^[1-9][0-9]*$ ]] || \
        fail "packaged shutdown worker PID file is malformed: $shutdown_worker_pid_path"
    if process_is_running "$SMOKE_WORKER_PID"; then
        fail "desktop shutdown coordinator left its active worker running: $SMOKE_WORKER_PID"
    fi
    grep -F "desktop shutdown completed: agents_seen=1 agents_closed=1 agents_already_exited=0 runtime_errors=0 daemon=Some(Spawned)" "$desktop_log" >/dev/null || \
        fail "desktop did not prove ordered worker, telemetry, and daemon shutdown; see $desktop_log"
    grep -F "daemon_ops=Some(DesktopDaemonOpsShutdownOutcome { worker_joined: true, lifecycle_outbox_drained: true, final_report_published: true, failures: [] })" "$desktop_log" >/dev/null || \
        fail "desktop did not report a successful typed daemon-ops shutdown outcome; see $desktop_log"
    grep -F "daemon_sidecar=Some(DesktopDaemonSidecarShutdownOutcome { mode: Spawned, terminate_reap: Reaped { pid:" "$desktop_log" >/dev/null || \
        fail "desktop did not report a confirmed typed sidecar reap outcome; see $desktop_log"
    SMOKE_WORKER_PID=""
    if kill -0 "$SMOKE_DAEMON_PID" 2>/dev/null; then
        fail "desktop-owned daemon survived ordered desktop shutdown: $SMOKE_DAEMON_PID"
    fi
    [[ ! -e "$socket_path" && ! -e "$daemon_pid_path" ]] || \
        fail "desktop-owned daemon left stale socket or PID state after shutdown"
    SMOKE_DAEMON_PID=""

    cleanup_smoke_processes
    trap - EXIT INT TERM
    echo "==> Packaged Dioxus smoke passed; evidence retained at $smoke_dir"
fi

echo "==> Verified Dioxus bundle: $APP_DIR"
