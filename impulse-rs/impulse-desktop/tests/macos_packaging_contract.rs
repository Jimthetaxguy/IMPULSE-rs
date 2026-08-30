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
    assert!(ci.contains("CARGO_TARGET_DIR: ${{ runner.temp }}/impulse-ci-package"));
    assert!(!ci.contains("CARGO_TARGET_DIR: target/"));
    assert!(!ci.contains("actions/upload-artifact"));

    assert!(candidate.contains("workflow_dispatch:"));
    assert!(candidate.contains("contents: read"));
    assert!(candidate.contains("build-macos-app.sh --universal --dmg"));
    assert!(candidate.contains("CARGO_TARGET_DIR: ${{ runner.temp }}/impulse-release-candidate"));
    assert!(candidate.contains(
        "shasum -a 256 \"$CARGO_TARGET_DIR\"/package/*non-distributable-developer-preview.dmg"
    ));
    assert!(!candidate.contains("CARGO_TARGET_DIR: target/"));
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

#[test]
fn active_package_recipe_requires_source_provenance_and_mounted_live_host_acceptance() {
    let workspace = workspace_root();
    let build = fs::read_to_string(workspace.join("scripts/build-macos-app.sh"))
        .expect("read macOS build script");
    let verify = fs::read_to_string(workspace.join("scripts/verify-macos-app.sh"))
        .expect("read macOS verifier");
    let provenance = fs::read_to_string(workspace.join("scripts/write-macos-provenance.sh"))
        .expect("read source-provenance writer");
    let live_host = fs::read_to_string(workspace.join("scripts/verify-packaged-host.sh"))
        .expect("read packaged-host verifier");

    assert!(build.contains("write-macos-provenance.sh"));
    assert!(build.contains("verify-packaged-host.sh"));
    assert!(build.contains("worktree add --detach"));
    assert!(build.contains("bash \"$SNAPSHOT_BUILD_SCRIPT\""));
    assert!(build.contains("worktree remove \"$SOURCE_SNAPSHOT\""));
    assert!(build.contains("path_is_within \"$1\" \"$2\" || path_is_within \"$2\" \"$1\""));
    assert!(build.contains("prepare_target_dir"));
    assert!(build.contains("prepare_target_child_dir"));
    assert!(build.contains("IMPULSE_MACOS_BUILD_OUTPUT_ROOT"));
    assert!(build.contains("cargo_build_target=\"$(mktemp -d"));
    assert!(build.contains("CARGO_TARGET_DIR=\"$cargo_build_target\""));
    assert!(build.contains("fresh Cargo build target must be empty before Cargo starts"));
    assert!(!build.contains("worktree remove --force"));
    assert!(!build.contains("rm -rf"));
    assert!(build.contains("ReleaseProvenance.v1.tsv"));
    assert!(build.contains("--source-root"));
    assert!(build.contains("$mount_dir/$APP_NAME.app"));
    assert!(build.contains("Embedded provenance SHA-256"));
    assert!(build.contains("DMG SHA-256"));
    assert!(build.contains("verify_mounted_volume_root"));
    assert!(build.contains("find \"$mount_root\" -mindepth 1 -maxdepth 1 -print0"));
    assert!(build.contains("mounted DMG root must contain exactly"));
    assert!(build.contains("FINAL_DMG_DIGEST"));
    assert!(build.contains("\"$FINAL_DMG_DIGEST\" == \"$DMG_DIGEST\""));
    assert!(build.matches("recheck_packaging_output_roots").count() >= 2);
    assert!(build.contains("cmp -s \"$STAGED_PROVENANCE\" \"$MOUNTED_PROVENANCE\""));
    assert!(build
        .contains("bash \"$LIVE_HOST_VERIFY_SCRIPT\" --source-root \"$PROTECTED_SOURCE_ROOT\""));
    assert!(!build.contains("bash \"$LIVE_HOST_VERIFY_SCRIPT\" --source-root \"$PROJECT_ROOT\""));
    assert!(verify.contains("ReleaseProvenance.v1.tsv"));
    assert!(verify.contains("--source-root"));
    assert!(provenance.contains("IMPULSE_RELEASE_PROVENANCE_V1"));
    assert!(provenance.contains("inventory_exclusion"));
    assert!(live_host.contains("packaged_live_host_acceptance"));
    assert!(live_host.contains("worktree add --detach"));
    assert!(live_host.contains("worktree remove \"$SOURCE_SNAPSHOT\""));
    assert!(live_host.contains("path_is_within \"$1\" \"$2\" || path_is_within \"$2\" \"$1\""));
    assert!(live_host.contains("prepare_target_dir"));
    assert!(live_host.contains("prepare_target_child_dir"));
    assert!(live_host.contains("packaged-host-runs"));
    assert!(live_host.contains("acceptance.XXXXXX"));
    assert!(!live_host.contains("$TARGET_BASE/packaged-host-acceptance"));
    assert!(!live_host.contains("worktree remove --force"));
    assert!(!live_host.contains("rm -rf"));
    assert!(live_host.contains("SNAPSHOT_BUNDLE_VERIFY_SCRIPT"));
    assert!(live_host.contains("$SNAPSHOT_WORKSPACE_ROOT/Cargo.toml"));
    assert!(live_host.contains("\"mounted app\" \"$APP_PATH\""));
    assert!(live_host.contains("IMPULSE_PACKAGED_APP_PATH"));
    assert!(live_host.contains("IMPULSE_PACKAGED_SOURCE_ROOT=\"$SOURCE_ROOT\""));
    assert!(!live_host.contains("IMPULSE_PACKAGED_ACCEPTANCE_CARGO_TARGET_DIR"));
    assert!(live_host.contains("verify-macos-app.sh"));
    assert!(live_host.matches("verify_source_unchanged").count() >= 3);
    let live_call = build
        .find("bash \"$LIVE_HOST_VERIFY_SCRIPT\" --source-root \"$PROTECTED_SOURCE_ROOT\"")
        .expect("build must call the mounted live-host verifier");
    assert!(
        build[live_call..].contains("verify_source_unchanged"),
        "build must recheck exact source after live-host verification"
    );
    assert!(!live_host.contains("host_readiness_smoke.mjs"));
    assert!(!live_host.contains("__IMPULSE_TEST_HOST_API"));
    assert!(!live_host.contains("__TAURI__"));
}

#[cfg(unix)]
fn write_executable_script(path: &Path, source: &str) {
    fs::write(path, source).expect("write executable script");
    make_executable(path);
}

#[cfg(unix)]
fn init_script_fixture(root: &Path, script_name: &str) -> (PathBuf, PathBuf) {
    let source = root.join("source");
    let workspace = source.join("impulse-rs");
    let scripts = workspace.join("scripts");
    fs::create_dir_all(&scripts).expect("create script fixture workspace");
    fs::write(workspace.join("Cargo.toml"), b"[workspace]\nmembers = []\n")
        .expect("write script fixture Cargo.toml");
    fs::write(
        workspace.join("Cargo.lock"),
        b"# deterministic script fixture\nversion = 3\n",
    )
    .expect("write script fixture Cargo.lock");
    fs::copy(
        workspace_root().join("scripts").join(script_name),
        scripts.join(script_name),
    )
    .expect("copy production script into fixture");
    make_executable(&scripts.join(script_name));

    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&source)
        .output()
        .expect("initialize script fixture Git repository");
    assert!(init.status.success());
    let add = Command::new("git")
        .args(["add", "impulse-rs/Cargo.toml", "impulse-rs/Cargo.lock"])
        .arg(format!("impulse-rs/scripts/{script_name}"))
        .current_dir(&source)
        .output()
        .expect("stage script fixture");
    assert!(add.status.success());
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=Impulse Packaging Tests",
            "-c",
            "user.email=impulse-packaging@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "script fixture",
        ])
        .current_dir(&source)
        .output()
        .expect("commit script fixture");
    assert!(
        commit.status.success(),
        "git commit fixture failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    (source, workspace)
}

#[cfg(unix)]
fn fake_cargo_path(root: &Path) -> (PathBuf, PathBuf) {
    let fake_bin = root.join("fake-bin");
    fs::create_dir_all(&fake_bin).expect("create fake command directory");
    let sentinel = root.join("cargo-ran");
    write_executable_script(
        &fake_bin.join("cargo"),
        "#!/bin/sh\nprintf 'cargo ran\\n' > \"$IMPULSE_CARGO_SENTINEL\"\nexit 99\n",
    );
    (fake_bin, sentinel)
}

#[cfg(unix)]
#[test]
fn detached_git_snapshot_executes_committed_bytes_and_cleans_up_normally() {
    let fixture = tempfile::tempdir().expect("create exact-snapshot fixture");
    let source = fixture.path().join("source");
    fs::create_dir(&source).expect("create exact-snapshot source");
    fs::write(source.join("sentinel.txt"), b"committed\n").expect("write committed sentinel");
    write_executable_script(
        &source.join("probe.sh"),
        "#!/bin/sh\nprintf '%s\\n' \"$PWD\"\ncat sentinel.txt\n",
    );
    assert!(Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&source)
        .status()
        .expect("initialize exact-snapshot Git fixture")
        .success());
    assert!(Command::new("git")
        .args(["add", "probe.sh", "sentinel.txt"])
        .current_dir(&source)
        .status()
        .expect("stage exact-snapshot fixture")
        .success());
    assert!(Command::new("git")
        .args([
            "-c",
            "user.name=Impulse Packaging Tests",
            "-c",
            "user.email=impulse-packaging@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "exact snapshot fixture",
        ])
        .current_dir(&source)
        .status()
        .expect("commit exact-snapshot fixture")
        .success());
    let commit = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&source)
            .output()
            .expect("resolve exact-snapshot commit")
            .stdout,
    )
    .expect("exact-snapshot commit is UTF-8")
    .trim()
    .to_string();
    let snapshot = fixture.path().join("detached-source");
    let add = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&snapshot)
        .arg(&commit)
        .current_dir(&source)
        .output()
        .expect("materialize detached exact-snapshot fixture");
    assert!(
        add.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    fs::write(source.join("sentinel.txt"), b"transient edit\n")
        .expect("write transient caller edit");
    let probe = Command::new("bash")
        .arg(snapshot.join("probe.sh"))
        .current_dir(&snapshot)
        .output()
        .expect("execute probe from detached exact snapshot");
    let canonical_snapshot = fs::canonicalize(&snapshot).expect("canonicalize detached snapshot");
    fs::write(source.join("sentinel.txt"), b"committed\n")
        .expect("restore caller bytes after transient edit");
    assert!(probe.status.success());
    let rendered = String::from_utf8(probe.stdout).expect("probe output is UTF-8");
    assert!(rendered.starts_with(&format!("{}\n", canonical_snapshot.display())));
    assert!(rendered.ends_with("committed\n"));

    let remove = Command::new("git")
        .args(["worktree", "remove"])
        .arg(&snapshot)
        .current_dir(&source)
        .output()
        .expect("remove clean detached exact-snapshot fixture");
    assert!(
        remove.status.success(),
        "normal worktree removal failed: {}",
        String::from_utf8_lossy(&remove.stderr)
    );
    assert!(!snapshot.exists());
    let worktrees = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&source)
        .output()
        .expect("list worktrees after cleanup");
    assert!(!String::from_utf8_lossy(&worktrees.stdout).contains("detached-source"));
}

#[cfg(unix)]
#[test]
fn build_rejects_an_overlapping_cargo_target_before_cargo_runs() {
    let fixture = tempfile::tempdir().expect("create build-target fixture");
    let (_source, workspace) = init_script_fixture(fixture.path(), "build-macos-app.sh");
    let (fake_bin, cargo_sentinel) = fake_cargo_path(fixture.path());
    let home = fixture.path().join("home");
    let scratch = fixture.path().join("scratch");
    fs::create_dir_all(&home).expect("create build-target home");
    fs::create_dir_all(&scratch).expect("create build-target scratch");
    let inherited_path = std::env::var("PATH").expect("PATH for script fixture");
    let output = Command::new("bash")
        .arg(workspace.join("scripts/build-macos-app.sh"))
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", fixture.path())
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run build target-overlap preflight");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("overlaps protected"),
        "unexpected build preflight stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before target rejection"
    );
}

#[cfg(unix)]
#[test]
fn build_rejects_a_symlinked_package_descendant_before_cargo_or_source_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("create package-descendant fixture");
    let (source, workspace) = init_script_fixture(fixture.path(), "build-macos-app.sh");
    let (fake_bin, cargo_sentinel) = fake_cargo_path(fixture.path());
    let home = fixture.path().join("home");
    let scratch = fixture.path().join("scratch");
    let target = fixture.path().join("external-target");
    fs::create_dir_all(&home).expect("create package-descendant home");
    fs::create_dir_all(&scratch).expect("create package-descendant scratch");
    fs::create_dir_all(&target).expect("create external target");
    symlink(&source, target.join("package")).expect("redirect package output into source");
    let manifest_before = fs::read(workspace.join("Cargo.toml")).expect("read source sentinel");
    let inherited_path = std::env::var("PATH").expect("PATH for script fixture");

    let output = Command::new("bash")
        .arg(workspace.join("scripts/build-macos-app.sh"))
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", &target)
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run package-descendant preflight");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("overlaps protected source worktree"),
        "unexpected package-descendant stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before descendant rejection"
    );
    assert_eq!(
        fs::read(workspace.join("Cargo.toml")).expect("reread source sentinel"),
        manifest_before,
        "symlinked package output mutated protected source"
    );
}

#[cfg(unix)]
#[test]
fn build_rejects_a_package_descendant_that_escapes_the_authorized_target() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("create package-escape fixture");
    let (_source, workspace) = init_script_fixture(fixture.path(), "build-macos-app.sh");
    let (fake_bin, cargo_sentinel) = fake_cargo_path(fixture.path());
    let home = fixture.path().join("home");
    let scratch = fixture.path().join("scratch");
    let target = fixture.path().join("authorized-target");
    let escaped = fixture.path().join("unrelated-shared-state");
    fs::create_dir_all(&home).expect("create package-escape home");
    fs::create_dir_all(&scratch).expect("create package-escape scratch");
    fs::create_dir_all(&target).expect("create authorized target");
    fs::create_dir_all(&escaped).expect("create unrelated shared state");
    let sentinel = escaped.join("sentinel.txt");
    fs::write(&sentinel, b"unchanged\n").expect("write escaped-state sentinel");
    symlink(&escaped, target.join("package")).expect("redirect package outside target");
    let inherited_path = std::env::var("PATH").expect("PATH for script fixture");

    let output = Command::new("bash")
        .arg(workspace.join("scripts/build-macos-app.sh"))
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", &target)
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run package-escape preflight");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("must remain within the Cargo target root"),
        "unexpected package-escape stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before escape rejection"
    );
    assert_eq!(
        fs::read(&sentinel).expect("reread escaped-state sentinel"),
        b"unchanged\n",
        "escaped package output mutated unrelated shared state"
    );
}

#[cfg(unix)]
#[test]
fn build_rejects_a_symlinked_fresh_cargo_parent_before_cargo_or_source_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("create fresh-Cargo parent fixture");
    let (source, workspace) = init_script_fixture(fixture.path(), "build-macos-app.sh");
    let (fake_bin, cargo_sentinel) = fake_cargo_path(fixture.path());
    let home = fixture.path().join("home");
    let scratch = fixture.path().join("scratch");
    let target = fixture.path().join("external-target");
    fs::create_dir_all(&home).expect("create Cargo-release home");
    fs::create_dir_all(&scratch).expect("create Cargo-release scratch");
    fs::create_dir_all(&target).expect("create external target");
    symlink(&source, target.join("cargo-builds")).expect("redirect fresh Cargo parent into source");
    let manifest_before = fs::read(workspace.join("Cargo.toml")).expect("read source sentinel");
    let inherited_path = std::env::var("PATH").expect("PATH for script fixture");

    let output = Command::new("bash")
        .arg(workspace.join("scripts/build-macos-app.sh"))
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", &target)
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run fresh-Cargo parent preflight");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("overlaps protected source worktree"),
        "unexpected fresh-Cargo parent stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before fresh-target rejection"
    );
    assert_eq!(
        fs::read(workspace.join("Cargo.toml")).expect("reread source sentinel"),
        manifest_before,
        "symlinked fresh Cargo parent mutated protected source"
    );
}

#[cfg(unix)]
#[test]
fn packaged_host_rejects_a_symlinked_fresh_target_parent_before_cargo() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("create packaged-host fresh-target fixture");
    let (source, workspace) = init_script_fixture(fixture.path(), "verify-packaged-host.sh");
    let (fake_bin, cargo_sentinel) = fake_cargo_path(fixture.path());
    let home = fixture.path().join("home");
    let scratch = fixture.path().join("scratch");
    let target = fixture.path().join("external-target");
    let app = fixture.path().join("mounted/Impulse.app");
    fs::create_dir_all(&home).expect("create packaged-host home");
    fs::create_dir_all(&scratch).expect("create packaged-host scratch");
    fs::create_dir_all(&target).expect("create packaged-host target");
    fs::create_dir_all(&app).expect("create packaged-host app placeholder");
    symlink(&source, target.join("packaged-host-runs"))
        .expect("redirect packaged-host run parent into source");
    let source_before = fs::read(workspace.join("Cargo.toml")).expect("read source sentinel");
    let inherited_path = std::env::var("PATH").expect("PATH for script fixture");

    let output = Command::new("bash")
        .arg(workspace.join("scripts/verify-packaged-host.sh"))
        .args(["--source-root", source.to_str().expect("UTF-8 source")])
        .arg(&app)
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", &target)
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run packaged-host fresh-target preflight");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("overlaps protected source worktree"),
        "unexpected packaged-host fresh-target stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before fresh-target rejection"
    );
    assert_eq!(
        fs::read(workspace.join("Cargo.toml")).expect("reread source sentinel"),
        source_before,
        "packaged-host target redirect mutated source"
    );
}

#[cfg(unix)]
#[test]
fn packaged_host_rejects_an_overlapping_cargo_target_before_cargo_runs() {
    let fixture = tempfile::tempdir().expect("create acceptance-target fixture");
    let (source, workspace) = init_script_fixture(fixture.path(), "verify-packaged-host.sh");
    let (fake_bin, cargo_sentinel) = fake_cargo_path(fixture.path());
    let home = fixture.path().join("home");
    let scratch = fixture.path().join("scratch");
    let app = fixture.path().join("mounted/Impulse.app");
    fs::create_dir_all(&home).expect("create acceptance-target home");
    fs::create_dir_all(&scratch).expect("create acceptance-target scratch");
    fs::create_dir_all(&app).expect("create acceptance-target app placeholder");
    let inherited_path = std::env::var("PATH").expect("PATH for script fixture");
    let output = Command::new("bash")
        .arg(workspace.join("scripts/verify-packaged-host.sh"))
        .args([
            "--source-root",
            source.to_str().expect("UTF-8 fixture source"),
        ])
        .arg(&app)
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", source.join("cargo-target"))
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run acceptance target-overlap preflight");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("overlaps protected"),
        "unexpected packaged-host preflight stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before target rejection"
    );

    let app_overlap = Command::new("bash")
        .arg(workspace.join("scripts/verify-packaged-host.sh"))
        .args([
            "--source-root",
            source.to_str().expect("UTF-8 fixture source"),
        ])
        .arg(&app)
        .env("PATH", format!("{}:{inherited_path}", fake_bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &scratch)
        .env("CARGO_TARGET_DIR", &app)
        .env("IMPULSE_CARGO_SENTINEL", &cargo_sentinel)
        .output()
        .expect("run mounted-app target-overlap preflight");
    assert!(!app_overlap.status.success());
    assert!(
        String::from_utf8_lossy(&app_overlap.stderr).contains("mounted app"),
        "unexpected mounted-app preflight stderr: {}",
        String::from_utf8_lossy(&app_overlap.stderr)
    );
    assert!(
        !cargo_sentinel.exists(),
        "Cargo ran before mounted-app target rejection"
    );
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("read fixture mode").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make fixture executable");
}

#[cfg(unix)]
fn init_fixture_source(root: &Path) -> PathBuf {
    let source = root.join("source");
    fs::create_dir_all(&source).expect("create fixture source root");
    fs::write(
        source.join("Cargo.lock"),
        b"# deterministic packaging-contract fixture\nversion = 3\n",
    )
    .expect("write fixture Cargo.lock");
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&source)
        .output()
        .expect("initialize fixture Git repository");
    assert!(init.status.success(), "git init fixture failed");
    let add = Command::new("git")
        .args(["add", "Cargo.lock"])
        .current_dir(&source)
        .output()
        .expect("stage fixture Cargo.lock");
    assert!(add.status.success(), "git add fixture failed");
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=Impulse Packaging Contract",
            "-c",
            "user.email=impulse-packaging@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture source",
        ])
        .current_dir(&source)
        .output()
        .expect("commit fixture source");
    assert!(
        commit.status.success(),
        "git commit fixture failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    source
}

#[cfg(unix)]
fn run_fixture_provenance_writer(source: &Path, app: &Path) -> std::process::Output {
    Command::new("bash")
        .arg(workspace_root().join("scripts/write-macos-provenance.sh"))
        .arg("--source-root")
        .arg(source)
        .args(["--version", "0.1.0", "--target", "aarch64-apple-darwin"])
        .arg(app)
        .output()
        .expect("run source-provenance writer")
}

#[cfg(unix)]
fn write_fixture_provenance(source: &Path, app: &Path) {
    let output = run_fixture_provenance_writer(source, app);
    assert!(
        output.status.success(),
        "provenance writer failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn fixture_bundle() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().expect("create packaging fixture");
    let source = init_fixture_source(fixture.path());
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
    write_fixture_provenance(&source, &app);

    fixture
}

#[cfg(unix)]
fn run_structural_verifier(app: &Path) -> std::process::Output {
    let source = app
        .parent()
        .expect("fixture app must have a parent")
        .join("source");
    Command::new("bash")
        .arg(workspace_root().join("scripts/verify-macos-app.sh"))
        .args(["--structure-only", "--version", "0.1.0"])
        .arg("--source-root")
        .arg(source)
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
fn provenance_writer_is_deterministic_for_identical_source_and_payload() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let source = fixture.path().join("source");
    let manifest_path = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    let first = fs::read(&manifest_path).expect("read first provenance manifest");
    fs::remove_file(&manifest_path).expect("remove first provenance manifest");
    write_fixture_provenance(&source, &app);
    let second = fs::read(&manifest_path).expect("read second provenance manifest");

    assert_eq!(first, second);
}

#[cfg(unix)]
#[test]
fn provenance_writer_rejects_a_dirty_source_tree() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let source = fixture.path().join("source");
    let manifest_path = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    fs::remove_file(&manifest_path).expect("remove fixture provenance manifest");
    fs::write(source.join("untracked-source.txt"), b"dirty\n").expect("dirty fixture source");
    let output = run_fixture_provenance_writer(&source, &app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("clean Git worktree"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_missing_source_provenance_manifest() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let manifest = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    if manifest.exists() {
        fs::remove_file(&manifest).expect("remove fixture provenance manifest");
    }
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ReleaseProvenance.v1.tsv"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_noncanonical_provenance_manifest_mode() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let manifest = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    let mut permissions = fs::metadata(&manifest)
        .expect("read provenance manifest mode")
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&manifest, permissions).expect("change provenance manifest mode");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("manifest mode"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_noncanonical_bundle_directory_mode() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let resources = app.join("Contents/Resources");
    let mut permissions = fs::metadata(&resources)
        .expect("read bundle directory mode")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&resources, permissions).expect("change bundle directory mode");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("directory mode"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_an_unexpected_macos_payload() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let unexpected = app.join("Contents/MacOS/unmanifested-helper");
    fs::write(&unexpected, b"#!/bin/sh\nexit 0\n").expect("write unexpected helper");
    make_executable(&unexpected);
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_an_unexpected_app_root_payload() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::write(app.join("private-note.txt"), b"must not ship\n")
        .expect("write unexpected app-root payload");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected bundle payload"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_an_unexpected_app_root_directory() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::create_dir(app.join("private-cache")).expect("create unexpected app-root directory");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected bundle directory"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_payload_digest_mismatch() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::write(
        app.join("Contents/Resources/assets/impulse_crt.css"),
        b"changed\n",
    )
    .expect("mutate fixture asset");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_source_commit_mismatch() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let manifest_path = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    let manifest = fs::read_to_string(&manifest_path).expect("read fixture provenance manifest");
    let corrupted = manifest
        .lines()
        .map(|line| {
            if line.starts_with("source_commit\t") {
                "source_commit\t0000000000000000000000000000000000000000"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&manifest_path, corrupted).expect("corrupt source commit");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source commit"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_source_tree_mismatch() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let manifest_path = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    let manifest = fs::read_to_string(&manifest_path).expect("read fixture provenance manifest");
    let corrupted = manifest
        .lines()
        .map(|line| {
            if line.starts_with("source_tree\t") {
                "source_tree\t0000000000000000000000000000000000000000"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&manifest_path, corrupted).expect("corrupt source tree");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source tree"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_cargo_lock_mismatch() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    fs::write(
        fixture.path().join("source/Cargo.lock"),
        b"# changed after manifest generation\nversion = 3\n",
    )
    .expect("mutate fixture Cargo.lock");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Cargo.lock differs"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_missing_self_exclusion() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let manifest_path = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    let manifest = fs::read_to_string(&manifest_path).expect("read fixture provenance manifest");
    let corrupted = manifest
        .lines()
        .filter(|line| !line.starts_with("inventory_exclusion\t"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&manifest_path, corrupted).expect("remove manifest self-exclusion");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("self-exclusion"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_duplicate_manifest_record() {
    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let manifest_path = app
        .join("Contents/Resources")
        .join("ReleaseProvenance.v1.tsv");
    let manifest = fs::read_to_string(&manifest_path).expect("read fixture provenance manifest");
    let duplicate = manifest
        .lines()
        .find(|line| line.starts_with("file\tContents/Info.plist\t"))
        .expect("manifest contains Info.plist record");
    let corrupted = format!("{manifest}{duplicate}\n");
    fs::write(&manifest_path, corrupted).expect("append duplicate record");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected manifest record"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_payload_mode_mismatch() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let asset = app.join("Contents/Resources/assets/impulse_crt.css");
    let mut permissions = fs::metadata(&asset).expect("read asset mode").permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&asset, permissions).expect("change asset mode");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("mode"));
}

#[cfg(unix)]
#[test]
fn structural_verifier_rejects_a_symlinked_payload() {
    use std::os::unix::fs::symlink;

    let fixture = fixture_bundle();
    let app = fixture.path().join("Impulse.app");
    let asset = app.join("Contents/Resources/assets/impulse_crt.css");
    let target = app.join("Contents/Resources/ReleaseCandidateNotice.txt");
    fs::remove_file(&asset).expect("remove fixture asset");
    symlink(&target, &asset).expect("symlink fixture asset");
    let output = run_structural_verifier(&app);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("regular file"));
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
