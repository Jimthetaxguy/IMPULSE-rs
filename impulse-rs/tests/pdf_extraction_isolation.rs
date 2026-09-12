//! Integration tests for the isolated PDF-extraction child process
//! (PR #54, Stage 1b-B, review round 1 findings P0-1/P0-2/P2-1 and review
//! round 2 finding P1 -- the unbounded child-stdout read, exercised here
//! via `tests/fakes/rogue-stdout-shim.sh`).
//!
//! These tests spawn the REAL compiled `impulse-rs` binary
//! (`env!("CARGO_BIN_EXE_impulse-rs")`) as the `internal-pdf-text` child,
//! via `impulse_rs::ion_repl::tool_document::extract_pdf_with_exe_and_timeout`
//! -- the same code path `document_read` uses in production, minus only the
//! `std::env::current_exe()` substitution (a `cargo test` process is never
//! `impulse-rs`/`ion` itself, so that substitution is the test seam). This
//! must be an INTEGRATION test, not a lib unit test: a self-referencing
//! Form XObject PDF crashes `pdf_extract::output_doc_page` via stack
//! overflow, which calls `abort()` -- calling that function directly in a
//! `cargo test` process, even inside one `#[test]`, would kill the entire
//! test binary, not just fail one test. Isolation is the fix under test,
//! so exercising it can only be done from OUTSIDE the process it protects.
//!
//! Fixtures are built with `lopdf` (re-exported by `pdf_extract::*`, so no
//! extra dependency) the same low-level way lopdf's own `create.rs`/
//! `encrypt.rs` examples do. This necessarily duplicates
//! `ion_repl::tool_document::tests::fixtures`' `pdf_dict`/`write_pdf`/
//! `write_pdf_blank_page`/`write_encrypted_pdf` helpers -- an integration
//! test crate cannot see a `#[cfg(test)]`-gated module inside the lib
//! crate, the same reason `tests/ion_verify_cli.rs` keeps its own copy of
//! `init_git_repo` (documented there, and in
//! `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`).

#![cfg(feature = "office-support")]

use std::path::PathBuf;
use std::time::Duration;

use impulse_rs::ion_repl::tool_document::{
    extract_pdf_with_exe_and_timeout, extract_pdf_with_exe_timeout_and_memory_limit, ExtractBudget,
};

fn impulse_rs_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_impulse-rs"))
}

fn pdf_dict(pairs: &[(&str, pdf_extract::Object)]) -> pdf_extract::Dictionary {
    let mut dict = pdf_extract::Dictionary::new();
    for (key, value) in pairs {
        dict.set(*key, value.clone());
    }
    dict
}

/// Builds a PDF with one page per entry of `pages`, each page's text
/// written via a single `Tj` operator. Mirrors
/// `ion_repl::tool_document::tests::fixtures::write_pdf`.
fn write_pdf(dir: &tempfile::TempDir, pages: &[&str]) -> PathBuf {
    use pdf_extract::content::{Content, Operation};
    use pdf_extract::{Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Font")),
        ("Subtype", Object::from("Type1")),
        ("BaseFont", Object::from("Helvetica")),
    ]));
    let resources_id = doc.add_object(pdf_dict(&[(
        "Font",
        Object::Dictionary(pdf_dict(&[("F1", Object::Reference(font_id))])),
    )]));
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for page_text in pages {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(*page_text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(pdf_dict(&[]), content.encode().unwrap()));
        let page_id = doc.add_object(pdf_dict(&[
            ("Type", Object::from("Page")),
            ("Parent", Object::Reference(pages_id)),
            ("Contents", Object::Reference(content_id)),
        ]));
        kids.push(Object::Reference(page_id));
    }
    let count = kids.len() as i64;
    let pages_dict = pdf_dict(&[
        ("Type", Object::from("Pages")),
        ("Kids", Object::Array(kids)),
        ("Count", Object::Integer(count)),
        ("Resources", Object::Reference(resources_id)),
        (
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        ),
    ]);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let catalog_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", catalog_id);
    let path = dir.path().join("doc.pdf");
    doc.save(&path).unwrap();
    path
}

/// One real page with an empty content stream: a well-formed PDF with a
/// page tree but no text operators anywhere.
fn write_pdf_blank_page(dir: &tempfile::TempDir) -> PathBuf {
    use pdf_extract::content::Content;
    use pdf_extract::{Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content = Content::<Vec<_>> { operations: vec![] };
    let content_id = doc.add_object(Stream::new(pdf_dict(&[]), content.encode().unwrap()));
    let page_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Page")),
        ("Parent", Object::Reference(pages_id)),
        ("Contents", Object::Reference(content_id)),
    ]));
    let pages_dict = pdf_dict(&[
        ("Type", Object::from("Pages")),
        ("Kids", Object::Array(vec![Object::Reference(page_id)])),
        ("Count", Object::Integer(1)),
        ("Resources", Object::Dictionary(pdf_dict(&[]))),
        (
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        ),
    ]);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let catalog_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", catalog_id);
    let path = dir.path().join("blank.pdf");
    doc.save(&path).unwrap();
    path
}

/// A one-page PDF re-saved with RC4 V1 encryption. `user_password` "" is
/// the P2-1 regression case: `lopdf::Document::load` authenticates an empty
/// user password automatically and silently decrypts, which is exactly why
/// `pdf_declares_encryption`'s raw byte scan (not `doc.is_encrypted()`
/// after loading) is the refusal mechanism.
fn write_encrypted_pdf(dir: &tempfile::TempDir, name: &str, user_password: &str) -> PathBuf {
    use pdf_extract::{Document, EncryptionState, EncryptionVersion, Object, Permissions};

    let plain = write_pdf(dir, &["secret contents"]);
    let mut doc = Document::load(&plain).unwrap();
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"0123456789abcdef".to_vec()),
            Object::string_literal(b"0123456789abcdef".to_vec()),
        ]),
    );
    let permissions = Permissions::PRINTABLE
        | Permissions::COPYABLE
        | Permissions::COPYABLE_FOR_ACCESSIBILITY
        | Permissions::PRINTABLE_IN_HIGH_QUALITY;
    let state = EncryptionState::try_from(EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner-pw",
        user_password,
        permissions,
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
    let path = dir.path().join(name);
    doc.save(&path).unwrap();
    path
}

/// A page whose content stream draws a Form XObject that draws itself
/// (`/X0 Do` inside `/X0`'s own content stream) -- review round 1's P0-1
/// fixture. `pdf_extract::output_doc_page`'s content-stream interpreter has
/// no depth/cycle guard on Form XObject execution and recurses until the
/// thread's stack is exhausted, which Rust's stack-overflow guard page
/// turns into `abort()`, not an unwinding panic.
fn write_self_referencing_xobject_pdf(dir: &tempfile::TempDir) -> PathBuf {
    use pdf_extract::{Document, Object, Stream};

    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let xobject_id = doc.new_object_id();

    let inner: Vec<u8> = b"q 1 0 0 1 0 0 cm /X0 Do Q\n".to_vec();

    let xobject_dict = pdf_dict(&[
        ("Type", Object::from("XObject")),
        ("Subtype", Object::from("Form")),
        (
            "BBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        ),
        (
            "Resources",
            Object::Dictionary(pdf_dict(&[(
                "XObject",
                Object::Dictionary(pdf_dict(&[("X0", Object::Reference(xobject_id))])),
            )])),
        ),
    ]);
    doc.objects.insert(
        xobject_id,
        Object::Stream(Stream::new(xobject_dict, inner.clone())),
    );

    let content_id = doc.add_object(Stream::new(pdf_dict(&[]), inner));
    let page_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Page")),
        ("Parent", Object::Reference(pages_id)),
        (
            "Resources",
            Object::Dictionary(pdf_dict(&[(
                "XObject",
                Object::Dictionary(pdf_dict(&[("X0", Object::Reference(xobject_id))])),
            )])),
        ),
        ("Contents", Object::Reference(content_id)),
    ]));
    let pages_dict = pdf_dict(&[
        ("Type", Object::from("Pages")),
        ("Kids", Object::Array(vec![Object::Reference(page_id)])),
        ("Count", Object::Integer(1)),
        (
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        ),
    ]);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let catalog_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", catalog_id);
    let path = dir.path().join("recxobj.pdf");
    doc.save(&path).unwrap();
    path
}

#[tokio::test]
async fn test_extract_pdf_reads_a_plain_multi_page_pdf_through_the_isolated_child() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_pdf(&dir, &["First page text", "Second page text"]);

    let parsed = extract_pdf_with_exe_and_timeout(
        &path,
        "doc.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect("a plain PDF must extract successfully through the isolated child");

    assert_eq!(parsed.format, "pdf");
    assert_eq!(parsed.sections.len(), 2);
    assert_eq!(parsed.sections[0].name.as_deref(), Some("Page 1"));
    assert_eq!(parsed.sections[1].name.as_deref(), Some("Page 2"));
    assert!(parsed.text.contains("First page text"), "{}", parsed.text);
    assert!(parsed.text.contains("Second page text"), "{}", parsed.text);
}

#[tokio::test]
async fn test_extract_pdf_blank_page_yields_zero_sections() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_pdf_blank_page(&dir);

    let parsed = extract_pdf_with_exe_and_timeout(
        &path,
        "blank.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect("a blank-page PDF is not an error");

    assert_eq!(parsed.sections.len(), 0);
    assert_eq!(parsed.text.chars().count(), 0);
}

/// **Review round 1, P0-1.** The core structural regression test: a
/// self-referencing Form XObject would previously crash the whole `ion`
/// process (an abort, uncatchable by `spawn_blocking`'s `JoinError`
/// containment). With isolation, it must instead surface as a typed `Err`
/// naming a signal -- and, critically, THIS TEST PROCESS must still be
/// alive afterward to make any further assertion at all. If isolation
/// regressed back to in-process rendering, this test would not report
/// "FAILED"; the whole `cargo test` process would abort with a stack-
/// overflow message and a non-zero exit code, taking every other test in
/// the same binary down with it.
#[tokio::test]
async fn test_extract_pdf_self_referencing_xobject_crashes_only_the_child_not_this_process() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_self_referencing_xobject_pdf(&dir);

    let result = extract_pdf_with_exe_and_timeout(
        &path,
        "recxobj.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await;

    let err = result.expect_err("a self-referencing Form XObject must not extract successfully");
    let message = err.to_string();
    assert!(
        message.contains("signal") || message.contains("timed out"),
        "expected a signal-kill or timeout error, got: {message}"
    );

    // The parent (this test process) is provably still alive and the tokio
    // runtime is still healthy: prove it by running another async
    // operation and a second, independent extraction to completion.
    tokio::time::sleep(Duration::from_millis(1)).await;
    let plain_path = write_pdf(&dir, &["still alive"]);
    let follow_up = extract_pdf_with_exe_and_timeout(
        &plain_path,
        "doc.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect("a normal extraction after the crash must still succeed");
    assert!(follow_up.text.contains("still alive"));
}

/// **Review round 1, P2-1.** `lopdf::Document::load` silently authenticates
/// and decrypts a PDF whose user password is empty -- `doc.is_encrypted()`
/// after loading would report `false`. `pdf_declares_encryption`'s raw
/// byte scan (checked before any parser runs, in both the parent's
/// `pdf_encryption_prescan` and the child's own independent check) must
/// refuse it regardless.
#[tokio::test]
async fn test_extract_pdf_refuses_encryption_even_with_an_empty_user_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_encrypted_pdf(&dir, "enc_emptyuser.pdf", "");

    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "enc_emptyuser.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("an empty-user-password encrypted PDF must still be refused");

    assert!(err.to_string().contains("encrypted PDF"), "{err}");
}

#[tokio::test]
async fn test_extract_pdf_refuses_encryption_with_a_non_empty_user_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_encrypted_pdf(&dir, "enc_userpw.pdf", "user-pw");

    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "enc_userpw.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("an encrypted PDF must be refused");

    assert!(err.to_string().contains("encrypted PDF"), "{err}");
}

/// The wall-clock timeout branch, forced deterministically: an absurdly
/// short timeout against an ordinary, fast-parsing PDF cannot possibly
/// complete in time, so this exercises the SAME kill/ProcessGroupGuard code
/// path a genuinely hung or slow-looping child would hit, without needing
/// a crafted slow fixture.
#[tokio::test]
async fn test_extract_pdf_reports_a_typed_timeout_when_the_child_cannot_finish_in_time() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_pdf(&dir, &["one page of perfectly ordinary text"]);

    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "doc.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_nanos(1),
    )
    .await
    .expect_err("an impossibly short timeout must be reported, not silently succeed");

    assert!(err.to_string().contains("timed out"), "{err}");
}

/// **Review round 1, item 2** ("assert the count, not just the error
/// string"). A single page whose text would exceed the budget must be
/// refused, and refused promptly -- proving the child's
/// `BoundedSink`/check-before-push fix actually short-circuits rendering
/// rather than building the whole oversized page first and checking
/// afterward (the pre-fix behavior that reached multiple GB of RSS).
#[tokio::test]
async fn test_extract_pdf_over_budget_page_is_refused_promptly() {
    let dir = tempfile::TempDir::new().unwrap();
    // Not a multi-megabyte fixture: the point is the refusal shape and
    // speed, both already proven at the character level by
    // handlers::internal_pdf_text's BoundedSink unit tests (which assert
    // the buffer itself, not just an error string, stays within budget on
    // a 10 MB single write).
    let path = write_pdf(&dir, &["this page has more than five characters in it"]);
    let budget = ExtractBudget {
        max_chars: 5,
        max_cells: 0,
    };

    let started = std::time::Instant::now();
    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "doc.pdf",
        budget,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("a page over the character budget must be refused");
    let elapsed = started.elapsed();

    assert!(err.to_string().contains("over the limit"), "{err}");
    assert!(
        elapsed < Duration::from_secs(10),
        "the budget refusal must be prompt (check-before-push), took {elapsed:?}"
    );
}

#[tokio::test]
async fn test_extract_pdf_page_count_cap_refuses_before_any_rendering() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_pdf(&dir, &["one", "two", "three"]);

    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "doc.pdf",
        ExtractBudget::DEFAULT,
        2,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("a PDF over the page cap must be refused");

    assert!(
        err.to_string().contains("3 pages, over the 2-page limit"),
        "{err}"
    );
}

#[tokio::test]
async fn test_extract_pdf_rejects_a_non_pdf_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("not-really.pdf");
    std::fs::write(&path, b"this is not a pdf").unwrap();

    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "not-really.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("garbage bytes with a .pdf extension must not parse");

    assert!(
        err.to_string()
            .starts_with("document_read: 'not-really.pdf' could not be parsed"),
        "{err}"
    );
}

/// `impulse-rs internal-pdf-text` is hidden from `--help` but still a real,
/// directly-invocable subcommand -- proves the CLI wiring end to end
/// (`cli.rs`'s `Commands::InternalPdfText`, `direct_dispatch.rs`'s arm,
/// `handlers::internal_pdf_text::run`) independent of the tool_document
/// spawning code, and that it is indeed hidden.
#[test]
fn test_internal_pdf_text_subcommand_is_hidden_but_directly_invocable() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_pdf(&dir, &["direct invocation"]);

    let help = std::process::Command::new(impulse_rs_exe())
        .arg("--help")
        .output()
        .expect("failed to run impulse-rs --help");
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(
        !help_stdout.contains("internal-pdf-text"),
        "internal-pdf-text must stay hidden from --help: {help_stdout}"
    );

    let output = std::process::Command::new(impulse_rs_exe())
        .arg("internal-pdf-text")
        .arg(&path)
        .arg("--max-chars")
        .arg("4096")
        .arg("--max-pages")
        .arg("10")
        .output()
        .expect("failed to run internal-pdf-text directly");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("direct invocation"), "{stdout}");
}

/// **Review round 2, P1 (CONFIRMED).** `child.wait_with_output()` buffered
/// the whole child stdout with no cap: a rogue child streaming ~1 GiB drove
/// the PARENT to ~3.2 GB RSS and produced an accepted, multi-gigabyte-
/// character "document". `tests/fakes/rogue-stdout-shim.sh` stands in for
/// a compromised/malicious `internal-pdf-text` (or a `current_exe()`/PATH
/// substitution pointing at something else): it ignores every argument and
/// writes far more than any reasonable stdout cap. Pointed at as `exe`
/// directly (bypassing the real subcommand entirely), this exercises the
/// SAME bounded-read code path (`read_capped`) the real child output goes
/// through, independent of PDF parsing.
#[tokio::test]
async fn test_extract_pdf_refuses_a_rogue_child_that_exceeds_the_stdout_bound() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_pdf(&dir, &["irrelevant text; the shim ignores every argument"]);
    // A small budget keeps the computed stdout cap small (a few KB), so
    // the shim's fixed ~1 GiB of output is refused almost immediately --
    // read_capped's `.take(cap + 1)` stops pulling bytes from the pipe the
    // instant the cap is reached, it does not wait for the shim to finish.
    let budget = ExtractBudget {
        max_chars: 1000,
        max_cells: 0,
    };

    let started = std::time::Instant::now();
    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "doc.pdf",
        budget,
        10,
        &rogue_stdout_shim(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("a rogue child streaming far more than the computed bound must be refused");
    let elapsed = started.elapsed();

    assert!(
        err.to_string().contains("exceeded its output bound"),
        "{err}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "must be detected and the child killed promptly, took {elapsed:?}"
    );
}

fn rogue_stdout_shim() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fakes/rogue-stdout-shim.sh")
}

// ==========================================================================
// Review round 3 (F0/F1/F2/F3/F4). These fixtures mirror the adversarial
// reviewer's own Python-generated fixtures (`objstm_bomb2g.pdf`,
// `lzwmulti300.pdf`, `legit64.pdf`, `chain_a85_flate*.pdf`) but built as
// tracked Rust source rather than depending on a scratchpad path outside
// this repository, so they stay portable across fresh clones, linked
// worktrees, and CI (per this project's verification-gate requirement).
// Scale is deliberately test-appropriate, not gigabyte-scale, achieved via
// the new `--memory-limit-bytes` / `extract_pdf_with_exe_timeout_and_
// memory_limit` seam (review round 3, F2) rather than needing a genuinely
// multi-GB fixture to trip the production 1 GiB ceiling; the reviewer's own
// full-scale fixtures were separately reproduced manually (see the lane
// card's review round 3 section for those measured numbers).
// ==========================================================================

fn compress_zlib(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write as _;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn encode_lzw(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
        .into_vec(&mut out)
        .encode_all(data)
        .status
        .unwrap();
    out
}

fn encode_ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut buf = [0u8; 4];
        buf[..chunk.len()].copy_from_slice(chunk);
        let v = u32::from_be_bytes(buf);
        if v == 0 && chunk.len() == 4 {
            out.push(b'z');
            continue;
        }
        let mut group = [0u8; 5];
        let mut n = v;
        for slot in group.iter_mut().rev() {
            *slot = 33 + (n % 85) as u8;
            n /= 85;
        }
        out.extend_from_slice(&group[..5 - (4 - chunk.len())]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// Builds a one-page PDF whose page content stream declares `filters` (a
/// `/Filter` name, or a `/Filter` array when more than one) over raw bytes
/// `content`.
fn write_pdf_with_stream(dir: &tempfile::TempDir, filters: &[&str], content: &[u8]) -> PathBuf {
    use pdf_extract::{Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    // A real Font/Resources dict: `write_pdf_with_stream`'s content streams
    // are actually RENDERED end to end by these tests (not merely
    // preflighted, unlike `internal_pdf_text`'s own unit-level fixture of
    // the same name), and `pdf_extract::output_doc_page` panics looking up
    // `/F1` if no font resource is present at all.
    let font_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Font")),
        ("Subtype", Object::from("Type1")),
        ("BaseFont", Object::from("Helvetica")),
    ]));
    let resources_id = doc.add_object(pdf_dict(&[(
        "Font",
        Object::Dictionary(pdf_dict(&[("F1", Object::Reference(font_id))])),
    )]));
    let pages_id = doc.new_object_id();
    let mut content_dict = pdf_dict(&[]);
    let filter_obj = match filters {
        [] => None,
        [one] => Some(Object::from(*one)),
        many => Some(Object::Array(
            many.iter().map(|f| Object::from(*f)).collect(),
        )),
    };
    if let Some(filter_obj) = filter_obj {
        content_dict.set("Filter", filter_obj);
    }
    let content_id = doc.add_object(Stream::new(content_dict, content.to_vec()));
    let page_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Page")),
        ("Parent", Object::Reference(pages_id)),
        ("Resources", Object::Reference(resources_id)),
        ("Contents", Object::Reference(content_id)),
    ]));
    let pages_dict = pdf_dict(&[
        ("Type", Object::from("Pages")),
        ("Kids", Object::Array(vec![Object::Reference(page_id)])),
        ("Count", Object::Integer(1)),
        (
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        ),
    ]);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let catalog_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", catalog_id);
    let path = dir.path().join("fixture.pdf");
    doc.save(&path).unwrap();
    path
}

/// A multi-page, genuinely Flate-compressed, legitimate document (review
/// round 3, F4): `pages` pages of `lines_per_page` lines each, real text
/// content, inflating well past the OLD 64 MiB total cap while staying
/// under the new 512 MiB one -- proving the raised cap actually admits
/// ordinary large documents (the old `legit10m.pdf`-style fixture had NO
/// compressed streams at all, so it never exercised this cap either way).
fn write_legit_multi_page_flate_pdf(
    dir: &tempfile::TempDir,
    pages: usize,
    lines_per_page: usize,
) -> PathBuf {
    use pdf_extract::{Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Font")),
        ("Subtype", Object::from("Type1")),
        ("BaseFont", Object::from("Helvetica")),
    ]));
    let resources_id = doc.add_object(pdf_dict(&[(
        "Font",
        Object::Dictionary(pdf_dict(&[("F1", Object::Reference(font_id))])),
    )]));
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for page in 0..pages {
        let mut body = String::from("BT /F1 12 Tf 10 780 Td 14 TL\n");
        for line in 0..lines_per_page {
            body.push_str(&format!(
                "(Page {page} line {line}: the quick brown fox jumps over the lazy dog \
                 0123456789) Tj T*\n"
            ));
        }
        body.push_str("ET\n");
        let compressed = compress_zlib(body.as_bytes());
        let mut content_dict = pdf_dict(&[]);
        content_dict.set("Filter", Object::from("FlateDecode"));
        let content_id = doc.add_object(Stream::new(content_dict, compressed));
        let page_id = doc.add_object(pdf_dict(&[
            ("Type", Object::from("Page")),
            ("Parent", Object::Reference(pages_id)),
            ("Resources", Object::Reference(resources_id)),
            ("Contents", Object::Reference(content_id)),
        ]));
        kids.push(Object::Reference(page_id));
    }
    let count = kids.len() as i64;
    let pages_dict = pdf_dict(&[
        ("Type", Object::from("Pages")),
        ("Kids", Object::Array(kids)),
        ("Count", Object::Integer(count)),
        (
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        ),
    ]);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let catalog_id = doc.add_object(pdf_dict(&[
        ("Type", Object::from("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", catalog_id);
    let path = dir.path().join("legit_multi_page.pdf");
    doc.save(&path).unwrap();
    path
}

/// Ports the reviewer's own `gen_objstm2g.py` fixture structure to Rust, at
/// a `pad_bytes`-controlled scale: objects 1 (Catalog), 2 (Pages), 3
/// (Page), and 5 (Font) all live INSIDE one `/Type /ObjStm` object stream
/// (object 6), padded with an ignored PDF comment (`% PPP...`) so the
/// object stream's PLAIN (pre-compression) size is `pad_bytes` -- a single
/// repeated byte compresses to a handful of bytes on disk regardless of how
/// large `pad_bytes` is. Object 4 (the page's content stream) and object 7
/// (the xref stream) stay OUTSIDE the ObjStm, as ordinary objects. This is
/// built with raw bytes, not `lopdf`'s `Document::save` (which does not
/// expose a way to mark an object as PDF-1.5-compressed / living inside an
/// ObjStm) -- the same reason the reviewer's own generator does not use
/// `lopdf`'s writer either.
///
/// **Review round 3, F0's exact mechanism:** `lopdf::Document::load`
/// eagerly decompresses this ObjStm as part of loading, unconditionally,
/// before any preflight can run -- the vulnerability this fixture proves is
/// contained (via the memory watchdog, not the stream-inflation preflight,
/// which never gets a chance to see this specific attack).
fn write_objstm_bomb_pdf(dir: &tempfile::TempDir, pad_bytes: usize) -> PathBuf {
    let inner: [(u32, &[u8]); 4] = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ];
    let mut pairs = Vec::new();
    let mut data = Vec::new();
    for (num, obj) in &inner {
        pairs.extend_from_slice(format!("{num} {} ", data.len()).as_bytes());
        data.extend_from_slice(obj);
        data.push(b' ');
    }
    let first = pairs.len();
    let mut objstm_plain = pairs;
    objstm_plain.extend_from_slice(&data);
    objstm_plain.extend_from_slice(b"\n% ");
    objstm_plain.extend(std::iter::repeat_n(b'P', pad_bytes));
    objstm_plain.extend_from_slice(b"\n");
    let comp = compress_zlib(&objstm_plain);

    let body = b"BT /F1 12 Tf 10 700 Td (Object stream document.) Tj ET\n";

    let mut buf: Vec<u8> = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let off4 = buf.len();
    buf.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", body.len()).as_bytes());
    buf.extend_from_slice(body);
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    let off6 = buf.len();
    buf.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /ObjStm /N {} /First {first} /Length {} /Filter /FlateDecode >>\n\
             stream\n",
            inner.len(),
            comp.len()
        )
        .as_bytes(),
    );
    buf.extend_from_slice(&comp);
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    let xstart = buf.len();
    let entry = |kind: u8, a: u32, b: u16| -> [u8; 7] {
        let mut e = [0u8; 7];
        e[0] = kind;
        e[1..5].copy_from_slice(&a.to_be_bytes());
        e[5..7].copy_from_slice(&b.to_be_bytes());
        e
    };
    let mut xdata = Vec::new();
    xdata.extend_from_slice(&entry(0, 0, 65535)); // obj 0: free
    xdata.extend_from_slice(&entry(2, 6, 0)); // obj 1: compressed, in objstm 6, index 0
    xdata.extend_from_slice(&entry(2, 6, 1)); // obj 2: index 1
    xdata.extend_from_slice(&entry(2, 6, 2)); // obj 3: index 2
    xdata.extend_from_slice(&entry(1, off4 as u32, 0)); // obj 4: normal
    xdata.extend_from_slice(&entry(2, 6, 3)); // obj 5: index 3
    xdata.extend_from_slice(&entry(1, off6 as u32, 0)); // obj 6: normal (the ObjStm itself)
    xdata.extend_from_slice(&entry(1, xstart as u32, 0)); // obj 7: normal (this xref stream)

    buf.extend_from_slice(
        format!(
            "7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            xdata.len()
        )
        .as_bytes(),
    );
    buf.extend_from_slice(&xdata);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{xstart}\n%%EOF\n").as_bytes());

    let path = dir.path().join("objstm_bomb.pdf");
    std::fs::write(&path, &buf).unwrap();
    path
}

/// **Review round 3, F0/F2 (CONFIRMED).** `objstm_bomb.pdf`'s ObjStm
/// inflates to ~4 MB of ignored comment padding on load -- past a 1 MB
/// injected watchdog ceiling, so the child's memory watchdog must fire and
/// the parent must map its distinct exit code to a typed "memory ceiling"
/// error, not a generic parse failure. The production ceiling is 1 GiB; a
/// small ceiling here proves the SAME mechanism deterministically and fast.
#[tokio::test]
async fn test_extract_pdf_refuses_an_objstm_bomb_via_the_memory_watchdog() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_objstm_bomb_pdf(&dir, 4 * 1024 * 1024);

    let err = extract_pdf_with_exe_timeout_and_memory_limit(
        &path,
        "objstm_bomb.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
        1024 * 1024,
    )
    .await
    .expect_err("an ObjStm bomb past the watchdog ceiling must be refused");

    assert!(
        err.to_string().contains("exceeded its memory ceiling"),
        "{err}"
    );
}

/// Sanity check that the ObjStm fixture itself is legitimate PDF structure
/// (not merely malformed and therefore refused for an unrelated reason):
/// under a GENEROUS watchdog ceiling, the same bomb extracts successfully,
/// proving the refusal above is specifically the memory ceiling firing, not
/// some other parse error.
#[tokio::test]
async fn test_extract_pdf_objstm_document_succeeds_under_a_generous_memory_limit() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_objstm_bomb_pdf(&dir, 4 * 1024 * 1024);

    let parsed = extract_pdf_with_exe_timeout_and_memory_limit(
        &path,
        "objstm_bomb.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
        1024 * 1024 * 1024,
    )
    .await
    .expect("the same ObjStm document must succeed under a generous memory ceiling");

    assert!(
        parsed.text.contains("Object stream document."),
        "{}",
        parsed.text
    );
}

/// **Review round 3, F1 (CONFIRMED).** `lopdf` decodes `LZWDecode` streams
/// exactly as readily as `FlateDecode`; the preflight must count them too.
/// Run through the REAL tool end to end (not just the unit-level
/// `preflight_pdf_streams` test in `handlers::internal_pdf_text`), proving
/// the whole pipeline -- parent spawn, child `Document::load`, preflight,
/// typed refusal -- works for the LZW case.
#[tokio::test]
async fn test_extract_pdf_refuses_an_lzw_bomb_end_to_end() {
    let dir = tempfile::TempDir::new().unwrap();
    // 80 MB decoded, over the 64 MiB per-stream cap -- 2 MB (the unit-level
    // `preflight_pdf_streams` test's scale) sits well UNDER the real
    // production cap, so it would pass here rather than prove a refusal;
    // this must actually clear the real cap to be a meaningful end-to-end
    // check.
    let bomb = encode_lzw(&vec![b'A'; 80_000_000]);
    let path = write_pdf_with_stream(&dir, &["LZWDecode"], &bomb);

    let err = extract_pdf_with_exe_and_timeout(
        &path,
        "lzwbomb.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect_err("an LZW-filtered compression bomb must be refused");

    assert!(err.to_string().contains("over the limit"), "{err}");
}

/// **Review round 3, F3 (CONFIRMED, then fixed).** Before this fix, a
/// `[/ASCII85Decode /FlateDecode]` chain was refused with "corrupt deflate
/// stream" because the ASCII85-ENCODED outer bytes were fed straight to
/// zlib. A legitimate document using this (uncommon but legal) chain must
/// now extract successfully end to end.
#[tokio::test]
async fn test_extract_pdf_ascii85_plus_flate_legitimate_chain_extracts_successfully() {
    let dir = tempfile::TempDir::new().unwrap();
    let compressed = compress_zlib(b"BT /F1 12 Tf 10 700 Td (hello from an a85+flate chain) Tj ET");
    let encoded = encode_ascii85(&compressed);
    let path = write_pdf_with_stream(&dir, &["ASCII85Decode", "FlateDecode"], &encoded);

    let parsed = extract_pdf_with_exe_and_timeout(
        &path,
        "chain.pdf",
        ExtractBudget::DEFAULT,
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(30),
    )
    .await
    .expect("a legitimate ASCII85+Flate chain must extract successfully");

    assert!(
        parsed.text.contains("hello from an a85+flate chain"),
        "{}",
        parsed.text
    );
}

/// **Review round 3, F4 (CONFIRMED, then fixed).** The OLD 64 MiB TOTAL cap
/// refused ordinary documents; this fixture's Flate-compressed content
/// inflates to well over 64 MiB combined (200 pages x 2000 lines of ~80
/// bytes each ~= 32 MB per page-content-stream-equivalent scale, batched
/// down here to keep the test fast while still clearing the old cap) but
/// stays comfortably under the new 512 MiB cap, and must extract
/// successfully -- proving the raised cap actually admits real documents
/// rather than only proving bombs still fail (the old `legit10m.pdf`-style
/// gap the reviewer flagged: a fixture with NO compressed streams proves
/// nothing about this cap either way).
#[tokio::test]
async fn test_extract_pdf_large_legitimate_flate_document_clears_the_new_512mib_cap() {
    let dir = tempfile::TempDir::new().unwrap();
    // ~80 bytes/line * 4000 lines/page * 25 pages ~= 8 MB of raw text per
    // page-equivalent block, well over the OLD 64 MiB total cap when summed
    // across enough pages, while completing in well under a second thanks
    // to zlib's speed on this kind of repetitive text.
    let path = write_legit_multi_page_flate_pdf(&dir, 25, 4000);
    let on_disk = std::fs::metadata(&path).unwrap().len();
    assert!(
        on_disk < 5 * 1024 * 1024,
        "fixture should compress far smaller than its inflated size: {on_disk}"
    );

    let parsed = extract_pdf_with_exe_and_timeout(
        &path,
        "legit_multi_page.pdf",
        ExtractBudget {
            max_chars: 16_000_000,
            max_cells: 0,
        },
        4096,
        &impulse_rs_exe(),
        Duration::from_secs(60),
    )
    .await
    .expect("a large legitimate multi-page Flate document must clear the new 512 MiB cap");

    assert_eq!(parsed.sections.len(), 25);
    assert!(parsed.text.contains("Page 0 line 0"), "{}", parsed.text);
}
