use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("impulse-desktop must live in the Cargo workspace")
        .to_path_buf()
}

fn repository_root() -> PathBuf {
    workspace_root()
        .parent()
        .expect("Cargo workspace must live in the repository")
        .to_path_buf()
}

#[test]
fn active_packaging_is_dioxus_owned_locked_and_non_publishing() {
    let workspace = workspace_root();
    let build = fs::read_to_string(workspace.join("scripts/build-macos-app.sh"))
        .expect("read macOS build script");
    let verify = fs::read_to_string(workspace.join("scripts/verify-macos-app.sh"))
        .expect("read macOS verification script");
    let ci = fs::read_to_string(repository_root().join(".github/workflows/ci.yml"))
        .expect("read CI workflow");
    let candidate = fs::read_to_string(repository_root().join(".github/workflows/release.yml"))
        .expect("read release-candidate workflow");

    assert!(build.contains("-p impulse-desktop --features desktop-app --bin impulse-desktop"));
    assert!(build.contains("-p impulse-rs --bin impulse-rs --bin ion"));
    assert!(build.contains("cargo build --locked --release"));
    assert!(build.contains("archive_existing"));
    assert!(build.contains("RUNTIME_ASSETS"));
    assert!(!build.contains("cp -R \"$DESKTOP_CRATE/assets\""));
    assert!(build.contains("ReleaseCandidateNotice.txt"));
    assert!(build.contains("verify-macos-app.sh"));
    assert!(build.contains("non-distributable-developer-preview"));
    assert!(build.contains("hdiutil attach"));
    assert!(!build.contains("impulse-gui"));
    assert!(!build.contains("rm -rf"));
    assert!(!build.contains("rm -f"));
    assert!(!build.contains("codesign"));
    assert!(!build.contains("notarytool"));

    assert!(verify.contains("DESKTOP_BIN=\"$MACOS_DIR/impulse-desktop\""));
    assert!(verify.contains("CONTROL_BIN=\"$MACOS_DIR/impulse-rs\""));
    assert!(verify.contains("ION_BIN=\"$MACOS_DIR/ion\""));
    assert!(verify.contains("ReleaseCandidateNotice.txt"));
    assert!(!verify.contains("impulse-gui"));
    assert!(!verify.contains("codesign"));

    assert!(ci.contains("cargo test --workspace --locked"));
    assert!(ci.contains("cargo check --workspace --all-targets --locked"));
    assert!(ci.contains("cargo clippy --workspace --all-targets --locked -- -D warnings"));
    assert!(ci.contains("needs: [test, lint, build]"));
    assert!(ci.contains(
        "cargo clippy -p impulse-desktop --features desktop-app --bin impulse-desktop --locked -- -D warnings"
    ));
    assert!(ci.contains("build-macos-app.sh --dmg"));
    assert!(!ci.contains("actions/upload-artifact"));

    assert!(candidate.contains("workflow_dispatch:"));
    assert!(candidate.contains("contents: read"));
    assert!(candidate.contains("build-macos-app.sh --universal --dmg"));
    assert!(candidate.contains(
        "cargo clippy -p impulse-desktop --features desktop-app --bin impulse-desktop --locked -- -D warnings"
    ));
    assert!(candidate.contains("non-distributable-developer-preview"));
    assert!(candidate.contains("--bin impulse-rs --bin ion"));
    assert!(candidate.contains("tar -C ../candidate -czf"));
    assert!(candidate.contains("test -x ../candidate/extracted/"));
    assert!(candidate.contains(".tar.gz"));
    assert!(!candidate.contains("actions/upload-artifact"));
    assert!(!candidate.contains("actions/download-artifact"));
    assert!(!candidate.contains("tags:"));
    assert!(!candidate.contains("softprops/action-gh-release"));
    assert!(!candidate.contains("contents: write"));
    assert!(!candidate.contains("impulse-gui"));
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
        crate_root.join("resources/ReleaseCandidateNotice.txt"),
        resources.join("ReleaseCandidateNotice.txt"),
    )
    .expect("copy release-candidate notice");

    for relative in [
        "assets/impulse_crt.css",
        "assets/vendor/xterm/xterm.css",
        "assets/vendor/xterm/xterm.js",
        "assets/vendor/xterm/addon-fit.js",
        "assets/vendor/xterm/manifest.json",
        "assets/vendor/xterm/LICENSE.xterm.txt",
        "assets/vendor/xterm/LICENSE.addon-fit.txt",
    ] {
        let path = resources.join(relative);
        fs::create_dir_all(path.parent().expect("fixture asset parent"))
            .expect("create fixture asset parent");
        fs::write(path, b"fixture\n").expect("write fixture asset");
    }

    for name in ["impulse-desktop", "impulse-rs", "ion"] {
        let binary = macos.join(name);
        fs::write(&binary, b"#!/bin/sh\nexit 0\n").expect("write fixture executable");
        make_executable(&binary);
    }

    fixture
}

#[cfg(unix)]
fn run_structural_verifier(app: &Path) -> std::process::Output {
    Command::new("bash")
        .arg(workspace_root().join("scripts/verify-macos-app.sh"))
        .args(["--structure-only", "--version", "0.1.0"])
        .arg(app)
        .output()
        .expect("run structural bundle verifier")
}

#[cfg(unix)]
#[test]
fn structural_verifier_accepts_a_portable_fixture() {
    let fixture = fixture_bundle();
    let output = run_structural_verifier(&fixture.path().join("Impulse.app"));

    assert!(
        output.status.success(),
        "verifier failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_missing_companion() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::remove_file(app.join("Contents/MacOS/impulse-rs")).expect("remove fixture companion");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("impulse-rs"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_missing_native_ion_sibling() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::remove_file(app.join("Contents/MacOS/ion")).expect("remove fixture Ion sibling");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ion"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_missing_runtime_asset() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::remove_file(app.join("Contents/Resources/assets/vendor/xterm/addon-fit.js"))
        .expect("remove fixture runtime asset");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("addon-fit.js"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_an_unexpected_runtime_asset() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::write(
        app.join("Contents/Resources/unexpected-private-note.txt"),
        b"must not ship\n",
    )
    .expect("write unexpected fixture resource");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected resource"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_legacy_bundle_metadata() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let plist_path = app.join("Contents/Info.plist");
    let plist = fs::read_to_string(&plist_path)
        .expect("read fixture plist")
        .replace("impulse-desktop", "impulse-gui");
    fs::write(&plist_path, plist).expect("write legacy fixture plist");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("CFBundleExecutable"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_bundle_signature_material() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let signature_dir = app.join("Contents/_CodeSignature");
    fs::create_dir_all(&signature_dir).expect("create fixture signature directory");
    fs::write(signature_dir.join("CodeResources"), b"fixture\n")
        .expect("write fixture signature material");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("bundle signature"));
}
