//! Integration tests for the isolated PDF-extraction child process
//! (review round 1, PR #54, Stage 1b-B, findings P0-1/P0-2/P2-1).
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

use impulse_rs::ion_repl::tool_document::{extract_pdf_with_exe_and_timeout, ExtractBudget};

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
/// `precheck_pdf` and the child's own independent check) must refuse it
/// regardless.
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
