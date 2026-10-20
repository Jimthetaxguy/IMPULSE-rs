// Sccache — setup and status for shared compilation cache
//
// sccache caches compiled artifacts across projects, so rebuilding
// after a `cargo clean` or switching between projects is much faster.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::storage::Storage;

const CARGO_CONFIG_ENTRY: &str = r#"[build]
rustc-wrapper = "sccache"
"#;

/// Status of sccache installation and configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SccacheStatus {
    /// Whether the sccache binary is installed
    pub installed: bool,
    /// sccache version string
    pub version: Option<String>,
    /// Whether ~/.cargo/config.toml is configured to use sccache
    pub configured: bool,
    /// Path to the cargo config file
    pub config_path: String,
    /// sccache cache stats (if running)
    pub stats: Option<SccacheStats>,
}

/// Basic sccache cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SccacheStats {
    pub cache_hits: Option<u64>,
    pub cache_misses: Option<u64>,
    pub cache_size: Option<String>,
}

/// Check the current status of sccache
pub fn sccache_status() -> SccacheStatus {
    let installed = is_sccache_installed();
    let version = if installed {
        get_sccache_version()
    } else {
        None
    };
    let config_path = cargo_config_path();
    let configured = config_path.as_deref().is_some_and(is_sccache_configured);
    let stats = if installed { get_sccache_stats() } else { None };

    SccacheStatus {
        installed,
        version,
        configured,
        config_path: config_path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        stats,
    }
}

/// Set up sccache as cargo's `rustc-wrapper` in the user's cargo config
/// (`$CARGO_HOME/config.toml`, by default `~/.cargo/config.toml`).
///
/// Adds `rustc-wrapper = "sccache"` under the existing `[build]` table, or
/// appends a `[build]` table, and preserves everything else. A wrapper that is
/// already set to something other than sccache is left alone and reported as
/// an error: a second `rustc-wrapper` key would make the file invalid TOML,
/// and cargo would then refuse every build that reads it.
pub fn sccache_setup(check_only: bool) -> Result<SccacheSetupResult> {
    if !is_sccache_installed() {
        bail!(
            "sccache is not installed. Install it with:\n  cargo install sccache\n  or: brew install sccache"
        );
    }

    let config_path = cargo_config_path().context(
        "Cannot locate the cargo config: neither CARGO_HOME nor a home directory is set",
    )?;
    let existing = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", config_path.display()))
        }
    };
    let display_path = config_path.to_string_lossy().to_string();

    let new_content = match plan_wrapper_edit(&existing)
        .with_context(|| format!("Not changing {}", config_path.display()))?
    {
        WrapperEdit::AlreadyConfigured => {
            return Ok(SccacheSetupResult {
                already_configured: true,
                config_path: display_path,
                action_taken: "Already configured".to_string(),
            });
        }
        WrapperEdit::Write(content) => content,
    };

    if check_only {
        return Ok(SccacheSetupResult {
            already_configured: false,
            config_path: display_path,
            action_taken: format!(
                "Not configured. Would add sccache wrapper to {}",
                config_path.display()
            ),
        });
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    Storage::atomic_write_path(&config_path, new_content.as_bytes())
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    Ok(SccacheSetupResult {
        already_configured: false,
        config_path: display_path,
        action_taken: format!(
            "Added sccache as rustc-wrapper in {}",
            config_path.display()
        ),
    })
}

/// What [`sccache_setup`] would do to a cargo config's text.
#[derive(Debug, PartialEq, Eq)]
enum WrapperEdit {
    AlreadyConfigured,
    Write(String),
}

/// How a cargo config sets `build.rustc-wrapper`.
#[derive(Debug, PartialEq, Eq)]
enum WrapperState {
    Unset,
    Sccache,
    /// Set to something else; the raw TOML value is kept for the message.
    Other(String),
    /// `build = { ... }` at the top level, which a `[build]` table would
    /// duplicate.
    InlineBuildTable,
}

/// Plans the edit without a TOML parser. Only the shapes cargo configs use in
/// practice are recognized: `[build]` headers (quoted or not, with optional
/// whitespace and a trailing comment), `rustc-wrapper` keys inside that table,
/// top-level dotted `build.*` keys, and an inline `build = { ... }`. Comment
/// lines, a leading byte-order mark, and the inside of multi-line strings
/// never count.
fn plan_wrapper_edit(existing: &str) -> Result<WrapperEdit> {
    match wrapper_state(existing) {
        WrapperState::Sccache => return Ok(WrapperEdit::AlreadyConfigured),
        WrapperState::Other(value) => {
            bail!("build.rustc-wrapper is already set to {value}; edit it by hand to use sccache")
        }
        WrapperState::InlineBuildTable => {
            bail!("build is an inline table; add rustc-wrapper = \"sccache\" to it by hand")
        }
        WrapperState::Unset => {}
    }
    if !is_simple_config(existing) {
        bail!(
            "the cargo config has multi-line values, which this edit cannot place a key \
             around safely; add rustc-wrapper = \"sccache\" under [build] (or \
             build.rustc-wrapper = \"sccache\" beside top-level build.* keys) by hand"
        );
    }

    let lines = scan_lines(existing);
    // The key goes under a `[build]` header if there is one. Otherwise, when
    // the top level already sets `build.*` with dotted keys, a `[build]`
    // table would define `build` twice and cargo would refuse the whole
    // file, so the key goes beside them in the same dotted form.
    let has_build_header = lines
        .iter()
        .any(|line| line.structural && table_header(line.text).as_deref() == Some("build"));
    let mut out = String::with_capacity(existing.len() + CARGO_CONFIG_ENTRY.len());
    let mut table = String::new();
    let mut inserted = false;
    for line in &lines {
        out.push_str(line.text);
        if inserted || !line.structural {
            continue;
        }
        let trimmed = content_of(line.text);
        let insertion = if trimmed.starts_with('[') {
            table = table_header(line.text).unwrap_or_else(|| "[[array]]".to_string());
            (table == "build").then_some("rustc-wrapper = \"sccache\"\n")
        } else if !has_build_header && table.is_empty() && line.ends_structural {
            trimmed
                .split_once('=')
                .filter(|(key, _)| normalize_key(key).starts_with("build."))
                .map(|_| "build.rustc-wrapper = \"sccache\"\n")
        } else {
            None
        };
        if let Some(entry) = insertion {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(entry);
            inserted = true;
        }
    }
    if !inserted {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(CARGO_CONFIG_ENTRY);
    }
    Ok(WrapperEdit::Write(out))
}

/// Whether the planner can edit `config` line by line: every line is blank,
/// a comment, a table header, or a `key = value` whose value ends on that
/// line. Three review rounds found shapes a line scanner without a TOML
/// parser placed the key inside (a multi-line array, a `"""` in a literal
/// string or a comment), and each one made cargo refuse the whole file, so
/// anything multi-line is now left for a hand edit.
fn is_simple_config(config: &str) -> bool {
    config.lines().all(|line| {
        let trimmed = content_of(line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return true;
        }
        if trimmed.starts_with('[') {
            return table_header(line).is_some() || trimmed.starts_with("[[");
        }
        !trimmed.contains(r#"""""#)
            && !trimmed.contains("'''")
            && trimmed
                .split_once('=')
                .is_some_and(|(_, value)| value_is_complete(value))
    })
}

/// A TOML value that closes on its own line: quotes closed, and `[`/`{`
/// balanced outside strings, stopping at a comment.
fn value_is_complete(value: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in value.chars() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if ch == '\\' => escaped = true,
            Some(open) if ch == open => quote = None,
            Some(_) => {}
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                '#' => break,
                _ => {}
            },
        }
    }
    quote.is_none() && depth == 0
}

/// One line of a config, newline included, and whether it is TOML
/// structure: a line that starts inside a multi-line string (`"""` or
/// `'''`) is string content, and one that ends inside one cannot take an
/// insertion after it.
struct ScannedLine<'a> {
    text: &'a str,
    structural: bool,
    ends_structural: bool,
}

fn scan_lines(config: &str) -> Vec<ScannedLine<'_>> {
    let mut open: Option<&str> = None;
    config
        .split_inclusive('\n')
        .map(|text| {
            let structural = open.is_none();
            if !(structural && content_of(text).starts_with('#')) {
                let mut rest = text;
                loop {
                    let next = match open {
                        Some(delimiter) => {
                            closing_delimiter(rest, delimiter).map(|at| (at, delimiter))
                        }
                        None => opening_delimiter(rest),
                    };
                    let Some((at, delimiter)) = next else { break };
                    rest = &rest[at + delimiter.len()..];
                    open = if open.is_some() {
                        None
                    } else {
                        Some(delimiter)
                    };
                }
            }
            ScannedLine {
                text,
                structural,
                ends_structural: open.is_none(),
            }
        })
        .collect()
}

/// Where a multi-line string opens in `rest`, outside a string: a `"""` or
/// `'''` inside a one-line `"..."` or `'...'`, or after a `#`, opens none.
/// Matching anywhere on the line hid everything after `X = '"""'` from the
/// wrapper check, which then reported a configured wrapper as unset.
fn opening_delimiter(rest: &str) -> Option<(usize, &'static str)> {
    let bytes = rest.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        for delimiter in [r#"""""#, "'''"] {
            if bytes[at..].starts_with(delimiter.as_bytes()) {
                return Some((at, delimiter));
            }
        }
        match bytes[at] {
            b'#' => return None,
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'\'' {
                    at += 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// Where the open multi-line string `delimiter` closes in `rest`. A basic
/// string (`"""`) may escape a quote with a backslash.
fn closing_delimiter(rest: &str, delimiter: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if delimiter == r#"""""# && bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if bytes[at..].starts_with(delimiter.as_bytes()) {
            return Some(at);
        }
        at += 1;
    }
    None
}

/// A line's text without surrounding whitespace or a byte-order mark.
fn content_of(line: &str) -> &str {
    line.trim().trim_start_matches('\u{feff}').trim()
}

fn wrapper_state(config: &str) -> WrapperState {
    // The table the current line belongs to; "" is the top level. An
    // array-of-tables header (`[[...]]`) never names `build`.
    let mut table = String::new();
    for line in scan_lines(config) {
        if !line.structural {
            continue;
        }
        let trimmed = content_of(line.text);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            table = table_header(line.text).unwrap_or_else(|| "[[array]]".to_string());
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        match (table.as_str(), normalize_key(key).as_str()) {
            ("build", "rustc-wrapper") | ("", "build.rustc-wrapper") => {
                return wrapper_value_state(value)
            }
            ("", "build") => return inline_build_state(value),
            _ => {}
        }
    }
    WrapperState::Unset
}

fn wrapper_value_state(value: &str) -> WrapperState {
    if names_sccache(wrapper_program(value)) {
        WrapperState::Sccache
    } else {
        WrapperState::Other(value.trim().to_string())
    }
}

/// `build = { ... }`: configured only if it sets `rustc-wrapper` to sccache.
fn inline_build_state(value: &str) -> WrapperState {
    let body = value
        .trim()
        .trim_start_matches('{')
        .split('}')
        .next()
        .unwrap_or_default();
    for pair in body.split(',') {
        if let Some((key, value)) = pair.split_once('=') {
            if normalize_key(key) == "rustc-wrapper" {
                return match wrapper_value_state(value) {
                    WrapperState::Sccache => WrapperState::Sccache,
                    _ => WrapperState::InlineBuildTable,
                };
            }
        }
    }
    WrapperState::InlineBuildTable
}

/// The program a `rustc-wrapper` value names, unquoted and without a
/// trailing comment: `"/opt/bin/sccache" # note` gives `/opt/bin/sccache`.
fn wrapper_program(value: &str) -> &str {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('"') {
        return rest.split('"').next().unwrap_or_default();
    }
    if let Some(rest) = value.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or_default();
    }
    value
        .split(|c: char| c.is_whitespace() || c == '#')
        .next()
        .unwrap_or_default()
}

/// Whether a program path's file name is sccache. A substring test also
/// accepted `/opt/no-sccache/wrap` and any value whose comment said sccache.
fn names_sccache(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    name == "sccache" || name == "sccache.exe"
}

/// The normalized name of a `[name]` table header line, ignoring whitespace,
/// quotes, a byte-order mark, and a trailing comment, so `[ "build" ]` is
/// `build`. Array-of-tables headers (`[[name]]`) return `None`.
fn table_header(line: &str) -> Option<String> {
    let rest = content_of(line).strip_prefix('[')?;
    if rest.starts_with('[') {
        return None;
    }
    let (name, after) = rest.split_once(']')?;
    let after = after.trim();
    (after.is_empty() || after.starts_with('#')).then(|| normalize_key(name))
}

/// A bare or dotted key with whitespace and quotes removed, so
/// `"build" . 'rustc-wrapper'` compares equal to `build.rustc-wrapper`.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
        .collect()
}

/// Result of sccache setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SccacheSetupResult {
    pub already_configured: bool,
    pub config_path: String,
    pub action_taken: String,
}

fn is_sccache_installed() -> bool {
    Command::new("sccache")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn get_sccache_version() -> Option<String> {
    Command::new("sccache")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

/// `$CARGO_HOME/config.toml`, falling back to `~/.cargo/config.toml` as
/// cargo itself does. `None` when neither location can be determined.
fn cargo_config_path() -> Option<PathBuf> {
    cargo_config_path_from(std::env::var_os("CARGO_HOME"), dirs::home_dir())
}

fn cargo_config_path_from(
    cargo_home: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let cargo_home = match cargo_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home?.join(".cargo"),
    };
    Some(cargo_home.join("config.toml"))
}

fn is_sccache_configured(config_path: &Path) -> bool {
    std::fs::read_to_string(config_path)
        .is_ok_and(|content| wrapper_state(&content) == WrapperState::Sccache)
}

fn get_sccache_stats() -> Option<SccacheStats> {
    let output = Command::new("sccache").arg("--show-stats").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut cache_hits = None;
    let mut cache_misses = None;
    let mut cache_size = None;

    for line in stdout.lines() {
        let lower = line.to_lowercase();
        if lower.contains("cache hit") {
            cache_hits = extract_number(line);
        } else if lower.contains("cache miss") {
            cache_misses = extract_number(line);
        } else if lower.contains("cache size") || lower.contains("cache_size") {
            cache_size = line.split_whitespace().last().map(|s| s.to_string());
        }
    }

    Some(SccacheStats {
        cache_hits,
        cache_misses,
        cache_size,
    })
}

fn extract_number(line: &str) -> Option<u64> {
    line.split_whitespace()
        .find_map(|word| word.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round 2 (reviewer C): a `"""` inside a one-line literal or a comment
    /// opened a multi-line string in the scanner, so the status check
    /// reported a configured wrapper as unset.
    #[test]
    fn test_wrapper_state_ignores_triple_quotes_in_one_line_strings_and_comments() {
        let wrapper = "[build]\nrustc-wrapper = \"sccache\"\n";
        for first in [
            "X = '\"\"\"'\n",
            "X = 1 # \"\"\"\n",
            "X = \"a \\\" \\\"\\\"\\\" b\"\n",
            "X = '''\nliteral '\n'''\n",
        ] {
            let config = format!("{first}{wrapper}");
            assert_eq!(wrapper_state(&config), WrapperState::Sccache, "{config}");
        }
        // A real multi-line string still hides what is inside it.
        let inside = format!("X = \"\"\"\n{wrapper}\"\"\"\n");
        assert_eq!(wrapper_state(&inside), WrapperState::Unset, "{inside}");
    }

    #[test]
    fn test_cargo_config_path_prefers_cargo_home() {
        let path =
            cargo_config_path_from(Some("/opt/cargo".into()), Some(PathBuf::from("/home/me")));
        assert_eq!(path, Some(PathBuf::from("/opt/cargo/config.toml")));
    }

    #[test]
    fn test_cargo_config_path_falls_back_to_home_dot_cargo() {
        let home = Some(PathBuf::from("/home/me"));
        let expected = Some(PathBuf::from("/home/me/.cargo/config.toml"));
        assert_eq!(cargo_config_path_from(None, home.clone()), expected);
        assert_eq!(cargo_config_path_from(Some("".into()), home), expected);
    }

    #[test]
    fn test_cargo_config_path_without_any_home_is_none() {
        assert_eq!(cargo_config_path_from(None, None), None);
    }

    #[test]
    fn test_plan_appends_a_build_table_to_an_empty_config() {
        assert_eq!(
            plan_wrapper_edit("").unwrap(),
            WrapperEdit::Write(CARGO_CONFIG_ENTRY.to_string())
        );
    }

    #[test]
    fn test_plan_appends_after_content_without_a_trailing_newline() {
        let WrapperEdit::Write(text) = plan_wrapper_edit("[net]\nretry = 3").unwrap() else {
            panic!("expected an edit");
        };
        assert_eq!(
            text,
            "[net]\nretry = 3\n[build]\nrustc-wrapper = \"sccache\"\n"
        );
    }

    #[test]
    fn test_plan_inserts_under_an_existing_build_table_once() {
        let existing = "[build] # shared settings\njobs = 8\n\n[term]\nverbose = true\n";
        let WrapperEdit::Write(text) = plan_wrapper_edit(existing).unwrap() else {
            panic!("expected an edit");
        };
        assert_eq!(
            text,
            "[build] # shared settings\nrustc-wrapper = \"sccache\"\njobs = 8\n\n[term]\nverbose = true\n"
        );
        assert_eq!(text.matches("rustc-wrapper").count(), 1);
    }

    /// The old string replace put a second `rustc-wrapper` key into
    /// `[build]`, which cargo rejects as duplicate-key TOML.
    #[test]
    fn test_plan_refuses_to_replace_a_different_wrapper() {
        let existing = "[build]\nrustc-wrapper = \"/usr/local/bin/other-cache\"\n";
        let error = plan_wrapper_edit(existing).unwrap_err().to_string();
        assert!(error.contains("other-cache"), "{error}");
    }

    #[test]
    fn test_plan_refuses_a_dotted_top_level_wrapper() {
        let existing = "build.rustc-wrapper = \"other\"\n[net]\nretry = 3\n";
        assert!(plan_wrapper_edit(existing).is_err());
    }

    #[test]
    fn test_plan_refuses_an_inline_build_table() {
        assert!(plan_wrapper_edit("build = { jobs = 8 }\n").is_err());
    }

    /// The old string replace also matched `[build]` inside a comment.
    #[test]
    fn test_plan_ignores_build_in_comments() {
        let existing = "# see [build] docs\n[net]\nretry = 3\n";
        let WrapperEdit::Write(text) = plan_wrapper_edit(existing).unwrap() else {
            panic!("expected an edit");
        };
        assert_eq!(
            text,
            "# see [build] docs\n[net]\nretry = 3\n[build]\nrustc-wrapper = \"sccache\"\n"
        );
    }

    #[test]
    fn test_plan_recognizes_an_existing_sccache_wrapper() {
        for existing in [
            "[build]\nrustc-wrapper = \"sccache\"\n",
            "[ build ]\n\"rustc-wrapper\" = \"/opt/homebrew/bin/sccache\"\n",
            "build.rustc-wrapper = \"sccache\"\n",
        ] {
            assert_eq!(
                plan_wrapper_edit(existing).unwrap(),
                WrapperEdit::AlreadyConfigured,
                "{existing}"
            );
        }
    }

    /// Refutation review of a42c5d4: each of these became a config cargo
    /// refuses ("duplicate key"), or the key landed inside a string.
    #[test]
    fn test_plan_keeps_the_config_one_valid_build_table() {
        for (existing, expected) in [
            (
                "build.jobs = 4\n[net]\nretry = 3\n",
                "build.jobs = 4\nbuild.rustc-wrapper = \"sccache\"\n[net]\nretry = 3\n",
            ),
            (
                "[\"build\"]\njobs = 4\n",
                "[\"build\"]\nrustc-wrapper = \"sccache\"\njobs = 4\n",
            ),
            (
                "\u{feff}[build]\njobs = 4\n",
                "\u{feff}[build]\nrustc-wrapper = \"sccache\"\njobs = 4\n",
            ),
        ] {
            let WrapperEdit::Write(text) = plan_wrapper_edit(existing).unwrap() else {
                panic!("expected an edit for {existing:?}");
            };
            assert_eq!(text, expected, "{existing:?}");
        }
    }

    /// Verification round on 25bf884: these were edited into configs cargo
    /// refuses (the key landed inside a multi-line array, or a `"""` in a
    /// literal string hid the real `[build]`). Multi-line values are now
    /// refused and left for a hand edit.
    #[test]
    fn test_plan_refuses_configs_with_multi_line_values() {
        for existing in [
            "build.rustflags = [\n  \"-C\", \"target-cpu=native\",\n]\n",
            "X = '\"\"\"'\n[build]\njobs = 4\n",
            "note = \"\"\"\n[build]\n\"\"\"\n",
        ] {
            let error = plan_wrapper_edit(existing).unwrap_err().to_string();
            assert!(error.contains("by hand"), "{existing:?}: {error}");
        }
    }

    #[test]
    fn test_value_is_complete_tracks_quotes_and_brackets() {
        assert!(value_is_complete(r#" ["a", "b"] # done"#));
        assert!(value_is_complete(r#" "x]\"y" "#));
        assert!(value_is_complete(" { jobs = 4 }"));
        assert!(!value_is_complete(" ["));
        assert!(!value_is_complete(r#" "unclosed"#));
    }

    /// Refutation review of a42c5d4: any value or comment containing
    /// "sccache" counted as configured.
    #[test]
    fn test_only_an_sccache_program_counts_as_configured() {
        for existing in [
            "[build]\nrustc-wrapper = \"/usr/local/bin/other\" # TODO switch to sccache\n",
            "[build]\nrustc-wrapper = \"/opt/no-sccache/wrap\"\n",
        ] {
            assert!(
                matches!(wrapper_state(existing), WrapperState::Other(_)),
                "{existing}"
            );
            assert!(plan_wrapper_edit(existing).is_err(), "{existing}");
        }
        assert_eq!(
            wrapper_state("build = { rustc-wrapper = \"sccache\" }\n"),
            WrapperState::Sccache
        );
        assert_eq!(
            wrapper_state("build = { rustc-wrapper = \"other\" }\n"),
            WrapperState::InlineBuildTable
        );
    }

    #[test]
    fn test_wrapper_in_another_table_does_not_count() {
        let existing = "[target.x86_64-unknown-linux-gnu]\nrustc-wrapper = \"sccache\"\n";
        assert_eq!(wrapper_state(existing), WrapperState::Unset);
    }

    #[test]
    fn test_sccache_status_structure() {
        let status = sccache_status();
        // Just verify the structure is valid
        assert!(!status.config_path.is_empty());
    }

    #[test]
    fn test_is_sccache_configured_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(!is_sccache_configured(&dir.path().join("config.toml")));
    }

    #[test]
    fn test_is_sccache_configured_without_entry() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "[profile.release]\nopt-level = 3\n").unwrap();
        assert!(!is_sccache_configured(tmp.path()));
    }

    #[test]
    fn test_is_sccache_configured_with_entry() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "[build]\nrustc-wrapper = \"sccache\"\n").unwrap();
        assert!(is_sccache_configured(tmp.path()));
    }

    #[test]
    fn test_is_sccache_configured_ignores_a_comment_mentioning_sccache() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "# sccache disabled for now\n[build]\njobs = 4\n",
        )
        .unwrap();
        assert!(!is_sccache_configured(tmp.path()));
    }

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("Cache hits: 42"), Some(42));
        assert_eq!(extract_number("No numbers here"), None);
    }
}
