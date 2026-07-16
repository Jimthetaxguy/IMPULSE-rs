use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("impulse-desktop must live under the Cargo workspace")
        .to_path_buf()
}

fn repository_root() -> PathBuf {
    workspace_root()
        .parent()
        .expect("Cargo workspace must live under the repository root")
        .to_path_buf()
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture source") {
        let entry = entry.expect("read fixture entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("read fixture file type").is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy fixture file");
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("read fixture mode").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make fixture executable");
}

#[cfg(unix)]
fn fixture_bundle() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().expect("create packaging fixture");
    let app = fixture.path().join("Impulse.app");
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos).expect("create fixture MacOS directory");
    fs::create_dir_all(&resources).expect("create fixture Resources directory");

    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plist = fs::read_to_string(crate_root.join("resources/Info.plist"))
        .expect("read Dioxus plist template")
        .replace("__VERSION__", "0.1.0");
    fs::write(contents.join("Info.plist"), plist).expect("write fixture plist");
    fs::copy(
        crate_root.join("resources/Impulse.icns"),
        resources.join("Impulse.icns"),
    )
    .expect("copy fixture icon");
    copy_tree(&crate_root.join("assets"), &resources.join("assets"));

    for name in ["impulse-desktop", "impulse-rs"] {
        let binary = macos.join(name);
        fs::write(&binary, b"#!/bin/sh\nexit 0\n").expect("write fixture executable");
        make_executable(&binary);
    }
    fixture
}

#[test]
fn test_active_packaging_targets_dioxus_and_archives_outputs() {
    let workspace = workspace_root();
    let build = fs::read_to_string(workspace.join("scripts/build-macos-app.sh"))
        .expect("read macOS build script");
    let verify = fs::read_to_string(workspace.join("scripts/verify-macos-app.sh"))
        .expect("read macOS verify script");
    let daemon_sidecar =
        fs::read_to_string(workspace.join("impulse-desktop/src/daemon_sidecar.rs"))
            .expect("read desktop daemon sidecar");
    let release = fs::read_to_string(repository_root().join(".github/workflows/release.yml"))
        .expect("read release workflow");

    assert!(build.contains("-p impulse-desktop --features desktop-app --bin impulse-desktop"));
    assert!(build.contains("-p impulse-rs --bin impulse-rs"));
    assert!(build.contains("archive_existing"));
    assert!(build.contains("Contents/Resources/assets"));
    assert!(build.contains("verify-macos-app.sh"));
    assert!(build.contains("package-staging"));
    assert!(build.contains("hdiutil attach"));
    assert!(build.contains("mount_dir/$APP_NAME.app"));
    assert!(build.contains("Ad-hoc signing post-build binaries and bundle"));
    assert!(build.contains("developer-preview.dmg"));
    assert!(!build.contains("impulse-gui"));
    assert!(!build.contains("rm -rf"));
    assert!(!build.contains("rm -f"));

    assert!(daemon_sidecar.contains("owned_daemon_command"));
    assert!(daemon_sidecar.contains(".arg(\"--owner-pid\")"));
    assert!(daemon_sidecar.contains("std::process::id()"));

    assert!(verify.contains("IMPULSE_DESKTOP_SMOKE=1"));
    assert!(verify.contains("IMPULSE_DESKTOP_SMOKE_RECEIPT "));
    assert!(verify.contains("IMPULSE_DESKTOP_SCOPE_PROBE=1"));
    assert!(verify.contains("IMPULSE_DESKTOP_SCOPE_RECEIPT "));
    assert!(verify.contains("no-env packaged desktop promoted an implicit daemon boundary"));
    assert!(
        verify.contains("scope resolution created project or memory state before user selection")
    );
    assert!(verify.contains("MACOS_DIR/impulse-desktop"));
    assert!(verify.contains("MACOS_DIR/impulse-rs"));
    assert!(verify.contains("/tmp/impulse-smoke-"));
    assert!(verify.contains("SUN_LEN"));
    assert!(verify.contains("desktop started packaged Impulse daemon companion"));
    assert!(verify.contains("desktop-shutdown-worker.pid"));
    assert!(verify.contains("agents_seen=1 agents_closed=1"));
    assert!(verify.contains("desktop shutdown coordinator left its active worker running"));
    assert!(verify.contains("DesktopDaemonOpsShutdownOutcome { worker_joined: true"));
    assert!(verify.contains("lifecycle_outbox_drained: true"));
    assert!(verify.contains("final_report_published: true"));
    assert!(verify.contains("DesktopDaemonSidecarShutdownOutcome { mode: Spawned"));
    assert!(verify.contains("terminate_reap: Reaped"));
    assert!(verify.contains("desktop-owned daemon survived ordered desktop shutdown"));
    assert!(!verify.contains("impulse-gui"));
    assert!(!verify.contains("rm -rf"));
    assert!(!release.contains("impulse-gui"));
    assert!(release.contains("build-macos-app.sh --universal --dmg --smoke"));
    assert!(release.contains("impulse-rs/target/package/Impulse-*.dmg"));
    assert!(release.contains("impulse-dioxus-developer-preview-dmg"));
}

#[test]
fn test_dioxus_bundle_metadata_names_the_real_host() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plist = fs::read_to_string(crate_root.join("resources/Info.plist"))
        .expect("read Dioxus plist template");
    let icon = fs::read(crate_root.join("resources/Impulse.icns")).expect("read Dioxus icon");

    assert!(plist.contains("<string>impulse-desktop</string>"));
    assert!(plist.contains("<string>com.impulse.ai</string>"));
    assert!(plist.contains("<string>Impulse.icns</string>"));
    assert!(!plist.contains("impulse-gui"));
    assert_eq!(&icon[..4], b"icns");
}

#[cfg(unix)]
#[test]
fn test_structural_verifier_accepts_a_portable_fixture() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let output = Command::new("bash")
        .arg(workspace_root().join("scripts/verify-macos-app.sh"))
        .args(["--structure-only", "--version", "0.1.0"])
        .arg(&app)
        .output()
        .expect("run structural bundle verifier");

    assert!(
        output.status.success(),
        "verifier failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn test_structural_verifier_rejects_a_missing_companion() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::remove_file(app.join("Contents/MacOS/impulse-rs")).expect("remove fixture companion");
    let output = Command::new("bash")
        .arg(workspace_root().join("scripts/verify-macos-app.sh"))
        .arg("--structure-only")
        .arg(&app)
        .output()
        .expect("run structural bundle verifier");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("impulse-rs"));
}
