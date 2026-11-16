//! Offline evaluation: current step-model selector vs a single best fixed model
//! vs choose-at-start. Eval only; it reads recorded runs and never calls or
//! changes `decide_step_model`.
//!
//! Source for the comparison: Handoff Curves (arXiv:2610.15993, abstract) asks
//! that every router be reported beside a simple fixed policy chosen on
//! training data.
//!
//! Input: JSONL, one record per run.
//!   {"task_id":"t1","split":"train","category":"refactor","policy":"fixed:sonnet",
//!    "pass":true,"cost_usd":0.12,"latency_ms":41000}
//! `policy` is either `current` (the run used the live selector, including any
//! escalation) or `fixed:<model>` (the whole task ran on one model from the
//! start). Fixed policies are fitted on `train`; every number printed is on
//! `test` only.
//!
//! Each policy is also compared task by task with a named reference policy, so
//! a report carries the fixed and regressed task lists that ADR-0016 rule 4
//! asks an evidence bundle to include. `newly passed` means the reference
//! failed the task and the policy passed it; `newly failed` (a regression)
//! means the reference passed the task and the policy failed it. A task with a
//! run on only one side is never counted as a gain or a regression; it is
//! listed separately as unmatched.
//!
//! The reference is chosen with `--reference <key>`:
//!   `current`          the live selector (default: the policy a candidate
//!                      must not regress against)
//!   `best-fixed`       the single best fixed model fitted on train
//!   `choose-at-start`  the per-category fixed model fitted on train
//!
//! Run: cargo run -p impulse-step-model --example selector_baselines -- runs.jsonl [--reference <key>]
//! Sample input: examples/data/selector_baselines_sample.jsonl

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
struct Run {
    task_id: String,
    split: String,
    #[serde(default)]
    category: String,
    policy: String,
    pass: bool,
    cost_usd: f64,
    latency_ms: f64,
}

/// Reference keys accepted by `--reference`, in report order.
const REFERENCE_KEYS: [&str; 3] = ["current", "best-fixed", "choose-at-start"];
const DEFAULT_REFERENCE: &str = "current";

#[derive(Debug, PartialEq)]
struct Report {
    /// Stable key used by `--reference`.
    key: String,
    name: String,
    tasks: usize,
    passes: usize,
    pass_rate: f64,
    cost_per_success: Option<f64>,
    p50_ms: f64,
    p95_ms: f64,
    /// Pass or fail per test task that has a matching run.
    results: BTreeMap<String, bool>,
}

/// Task-level difference between a policy and a reference policy.
#[derive(Debug, Default, PartialEq)]
struct Comparison {
    /// Failed by the reference, passed by the policy.
    newly_passed: Vec<String>,
    /// Passed by the reference, failed by the policy (regressions).
    newly_failed: Vec<String>,
    /// Run by the reference only; not counted as a gain or a regression.
    only_in_reference: Vec<String>,
    /// Run by the policy only; not counted as a gain or a regression.
    only_in_policy: Vec<String>,
}

/// Compares per-task results of `policy` against `reference`. Only tasks that
/// both sides ran can be newly passed or newly failed; one-sided tasks are
/// reported as unmatched so a missing run never reads as a fix or a
/// regression. Every list is sorted by task id.
fn compare_against_reference(
    reference: &BTreeMap<String, bool>,
    policy: &BTreeMap<String, bool>,
) -> Comparison {
    let mut c = Comparison::default();
    for (task, &ref_pass) in reference {
        match policy.get(task) {
            Some(&true) if !ref_pass => c.newly_passed.push(task.clone()),
            Some(&false) if ref_pass => c.newly_failed.push(task.clone()),
            Some(_) => {}
            None => c.only_in_reference.push(task.clone()),
        }
    }
    c.only_in_policy = policy
        .keys()
        .filter(|t| !reference.contains_key(*t))
        .cloned()
        .collect();
    c
}

/// Parses `<runs.jsonl> [--reference <key>]`; `--reference=<key>` also works.
fn parse_args(args: &[String]) -> Result<(String, String), String> {
    let usage = format!(
        "usage: selector_baselines <runs.jsonl> [--reference <{}>] (default: {DEFAULT_REFERENCE})",
        REFERENCE_KEYS.join("|")
    );
    let mut path = None;
    let mut reference = DEFAULT_REFERENCE.to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--reference" {
            reference = it.next().ok_or(&usage)?.clone();
        } else if let Some(v) = a.strip_prefix("--reference=") {
            reference = v.to_string();
        } else if a.starts_with("--") || path.is_some() {
            return Err(usage);
        } else {
            path = Some(a.clone());
        }
    }
    if !REFERENCE_KEYS.contains(&reference.as_str()) {
        return Err(format!("unknown reference policy `{reference}`; {usage}"));
    }
    Ok((path.ok_or(usage)?, reference))
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[i]
}

fn report(key: &str, name: &str, runs: &[&Run]) -> Report {
    let passes = runs.iter().filter(|r| r.pass).count();
    let cost: f64 = runs.iter().map(|r| r.cost_usd).sum();
    let mut lat: Vec<f64> = runs.iter().map(|r| r.latency_ms).collect();
    lat.sort_by(|a, b| a.total_cmp(b));
    Report {
        key: key.to_string(),
        name: name.to_string(),
        tasks: runs.len(),
        passes,
        pass_rate: if runs.is_empty() {
            0.0
        } else {
            passes as f64 / runs.len() as f64
        },
        cost_per_success: (passes > 0).then(|| cost / passes as f64),
        p50_ms: pct(&lat, 0.50),
        p95_ms: pct(&lat, 0.95),
        results: runs.iter().map(|r| (r.task_id.clone(), r.pass)).collect(),
    }
}

/// Best fixed model on a set of train runs: highest pass rate, then lower cost, then name.
fn best_fixed<'a>(train: impl Iterator<Item = &'a Run>) -> Option<String> {
    let mut by: BTreeMap<&str, (usize, usize, f64)> = BTreeMap::new();
    for r in train {
        if let Some(m) = r.policy.strip_prefix("fixed:") {
            let e = by.entry(m).or_default();
            e.0 += 1;
            e.1 += r.pass as usize;
            e.2 += r.cost_usd;
        }
    }
    by.into_iter()
        .max_by(|a, b| {
            let ra = a.1 .1 as f64 / a.1 .0 as f64;
            let rb = b.1 .1 as f64 / b.1 .0 as f64;
            ra.total_cmp(&rb)
                .then(b.1 .2.total_cmp(&a.1 .2))
                .then(b.0.cmp(a.0))
        })
        .map(|(m, _)| m.to_string())
}

fn evaluate(runs: &[Run]) -> (Vec<Report>, Vec<String>) {
    let mut notes = Vec::new();
    let train: Vec<&Run> = runs.iter().filter(|r| r.split == "train").collect();
    let test: Vec<&Run> = runs.iter().filter(|r| r.split == "test").collect();
    let test_tasks: BTreeSet<&str> = test.iter().map(|r| r.task_id.as_str()).collect();
    let pick = |policy: &str| -> Vec<&Run> {
        let mut out = Vec::new();
        for t in &test_tasks {
            if let Some(r) = test.iter().find(|r| r.task_id == *t && r.policy == policy) {
                out.push(*r);
            }
        }
        out
    };

    let mut reports = vec![report("current", "current selector", &pick("current"))];

    let global = best_fixed(train.iter().copied());
    match &global {
        Some(m) => reports.push(report(
            "best-fixed",
            &format!("single best fixed ({m}, fitted on train)"),
            &pick(&format!("fixed:{m}")),
        )),
        None => notes.push("no fixed-model train runs: single best fixed not reported".into()),
    }

    // Choose-at-start: one model per task, picked before the task starts from its
    // category, using the best fixed model for that category on train.
    let mut start = Vec::new();
    for t in &test_tasks {
        let cat = test
            .iter()
            .find(|r| r.task_id == *t)
            .map(|r| r.category.as_str())
            .unwrap_or("");
        let m = best_fixed(train.iter().copied().filter(|r| r.category == cat))
            .or_else(|| global.clone());
        if let Some(m) = m {
            if let Some(r) = test
                .iter()
                .find(|r| r.task_id == *t && r.policy == format!("fixed:{m}"))
            {
                start.push(*r);
            }
        }
    }
    reports.push(report(
        "choose-at-start",
        "choose-at-start (by category, fitted on train)",
        &start,
    ));

    for r in &reports {
        if r.tasks < test_tasks.len() {
            notes.push(format!(
                "{}: {} of {} test tasks have a matching run",
                r.name,
                r.tasks,
                test_tasks.len()
            ));
        }
    }
    (reports, notes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, reference) = parse_args(&args)?;
    let text = std::fs::read_to_string(path)?;
    let runs: Vec<Run> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let (reports, notes) = evaluate(&runs);
    let reference_report = reports
        .iter()
        .find(|r| r.key == reference)
        .ok_or_else(|| format!("reference policy `{reference}` was not reported for this input"))?;
    println!(
        "{:<52}{:>7}{:>9}{:>11}{:>14}{:>10}{:>10}",
        "policy (test split only)", "tasks", "pass", "regressed", "$/success", "p50 ms", "p95 ms"
    );
    for r in &reports {
        let cps = r
            .cost_per_success
            .map(|c| format!("{c:.4}"))
            .unwrap_or_else(|| "n/a".into());
        let regressed = if r.key == reference {
            "ref".to_string()
        } else {
            compare_against_reference(&reference_report.results, &r.results)
                .newly_failed
                .len()
                .to_string()
        };
        println!(
            "{:<52}{:>7}{:>8.1}%{:>11}{:>14}{:>10.0}{:>10.0}",
            r.name,
            r.tasks,
            100.0 * r.pass_rate,
            regressed,
            cps,
            r.p50_ms,
            r.p95_ms
        );
    }
    println!();
    println!(
        "Against reference `{reference}` ({}), test split only:",
        reference_report.name
    );
    for r in reports.iter().filter(|r| r.key != reference) {
        let c = compare_against_reference(&reference_report.results, &r.results);
        println!("{}:", r.name);
        println!(
            "  newly passed ({}): {}",
            c.newly_passed.len(),
            list(&c.newly_passed)
        );
        println!(
            "  newly failed ({}): {}",
            c.newly_failed.len(),
            list(&c.newly_failed)
        );
        if !c.only_in_reference.is_empty() || !c.only_in_policy.is_empty() {
            println!(
                "  unmatched, not counted: reference only {}; policy only {}",
                list(&c.only_in_reference),
                list(&c.only_in_policy)
            );
        }
    }
    for n in notes {
        println!("note: {n}");
    }
    println!("Cost includes escalation only where the recorded `current` runs include it.");
    Ok(())
}

fn list(tasks: &[String]) -> String {
    if tasks.is_empty() {
        "none".into()
    } else {
        tasks.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        task: &str,
        split: &str,
        cat: &str,
        policy: &str,
        pass: bool,
        cost: f64,
        lat: f64,
    ) -> Run {
        Run {
            task_id: task.into(),
            split: split.into(),
            category: cat.into(),
            policy: policy.into(),
            pass,
            cost_usd: cost,
            latency_ms: lat,
        }
    }

    fn data() -> Vec<Run> {
        vec![
            // train: sonnet best overall; opus best on "hard"
            run("a", "train", "easy", "fixed:sonnet", true, 1.0, 10.0),
            run("a", "train", "easy", "fixed:opus", true, 3.0, 20.0),
            run("b", "train", "hard", "fixed:sonnet", false, 1.0, 10.0),
            run("b", "train", "hard", "fixed:opus", true, 3.0, 20.0),
            run("c", "train", "easy", "fixed:sonnet", true, 1.0, 10.0),
            run("c", "train", "easy", "fixed:opus", false, 3.0, 20.0),
            // test
            run("x", "test", "easy", "current", true, 1.5, 12.0),
            run("x", "test", "easy", "fixed:sonnet", true, 1.0, 10.0),
            run("x", "test", "easy", "fixed:opus", true, 3.0, 20.0),
            run("y", "test", "hard", "current", false, 2.0, 30.0),
            run("y", "test", "hard", "fixed:sonnet", false, 1.0, 10.0),
            run("y", "test", "hard", "fixed:opus", true, 3.0, 20.0),
        ]
    }

    #[test]
    fn fixed_policy_is_fitted_on_train_only() {
        let d = data();
        assert_eq!(
            best_fixed(d.iter().filter(|r| r.split == "train")),
            Some("sonnet".into())
        );
    }

    #[test]
    fn three_policies_reported_on_test_split() {
        let (r, notes) = evaluate(&data());
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].tasks, r[0].passes), (2, 1));
        assert!(r[1].name.contains("sonnet"));
        assert_eq!(r[1].passes, 1);
        // choose-at-start picks sonnet for easy, opus for hard
        assert_eq!(r[2].passes, 2);
        assert_eq!(r[2].cost_per_success, Some(2.0));
    }

    #[test]
    fn missing_runs_are_noted_not_hidden() {
        let mut d = data();
        d.retain(|r| !(r.task_id == "y" && r.policy == "current"));
        let (_, notes) = evaluate(&d);
        assert!(notes.iter().any(|n| n.contains("current selector: 1 of 2")));
    }

    fn results(pairs: &[(&str, bool)]) -> BTreeMap<String, bool> {
        pairs.iter().map(|(t, p)| (t.to_string(), *p)).collect()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn compare_both_sides_empty() {
        let c = compare_against_reference(&BTreeMap::new(), &BTreeMap::new());
        assert_eq!(c, Comparison::default());
    }

    #[test]
    fn compare_identical_results() {
        let r = results(&[("a", true), ("b", false), ("c", true)]);
        assert_eq!(compare_against_reference(&r, &r), Comparison::default());
    }

    #[test]
    fn compare_only_gains() {
        let reference = results(&[("a", false), ("b", false), ("c", true)]);
        let policy = results(&[("a", true), ("b", true), ("c", true)]);
        let c = compare_against_reference(&reference, &policy);
        assert_eq!(c.newly_passed, ids(&["a", "b"]));
        assert!(c.newly_failed.is_empty());
    }

    #[test]
    fn compare_only_regressions() {
        let reference = results(&[("a", true), ("b", true), ("c", false)]);
        let policy = results(&[("a", false), ("b", false), ("c", false)]);
        let c = compare_against_reference(&reference, &policy);
        assert!(c.newly_passed.is_empty());
        assert_eq!(c.newly_failed, ids(&["a", "b"]));
    }

    #[test]
    fn compare_mix_is_sorted() {
        let reference = results(&[("d", true), ("a", false), ("c", true), ("b", false)]);
        let policy = results(&[("d", false), ("a", true), ("c", true), ("b", false)]);
        let c = compare_against_reference(&reference, &policy);
        assert_eq!(c.newly_passed, ids(&["a"]));
        assert_eq!(c.newly_failed, ids(&["d"]));
        assert!(c.only_in_reference.is_empty() && c.only_in_policy.is_empty());
    }

    #[test]
    fn compare_one_sided_tasks_are_unmatched_not_counted() {
        let reference = results(&[("a", true), ("r", true), ("s", false)]);
        let policy = results(&[("a", true), ("p", false), ("q", true)]);
        let c = compare_against_reference(&reference, &policy);
        assert!(c.newly_passed.is_empty());
        assert!(c.newly_failed.is_empty());
        assert_eq!(c.only_in_reference, ids(&["r", "s"]));
        assert_eq!(c.only_in_policy, ids(&["p", "q"]));
    }

    #[test]
    fn evaluated_reports_compare_against_current() {
        let (r, _) = evaluate(&data());
        let keys: Vec<&str> = r.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, REFERENCE_KEYS);
        // choose-at-start passes y, which the current selector fails.
        let c = compare_against_reference(&r[0].results, &r[2].results);
        assert_eq!(c.newly_passed, ids(&["y"]));
        assert!(c.newly_failed.is_empty());
        // Against choose-at-start, the current selector regresses on y.
        let c = compare_against_reference(&r[2].results, &r[0].results);
        assert_eq!(c.newly_failed, ids(&["y"]));
    }

    #[test]
    fn args_default_and_selected_reference() {
        let a = |v: &[&str]| parse_args(&ids(v));
        assert_eq!(
            a(&["runs.jsonl"]),
            Ok(("runs.jsonl".into(), "current".into()))
        );
        assert_eq!(
            a(&["runs.jsonl", "--reference", "best-fixed"]),
            Ok(("runs.jsonl".into(), "best-fixed".into()))
        );
        assert_eq!(
            a(&["--reference=choose-at-start", "runs.jsonl"]),
            Ok(("runs.jsonl".into(), "choose-at-start".into()))
        );
        assert!(a(&["runs.jsonl", "--reference", "opus"]).is_err());
        assert!(a(&["runs.jsonl", "--reference"]).is_err());
        assert!(a(&[]).is_err());
        assert!(a(&["a.jsonl", "b.jsonl"]).is_err());
    }

    #[test]
    fn percentiles() {
        assert_eq!(pct(&[1.0, 2.0, 3.0, 4.0], 0.5), 2.0);
        assert_eq!(pct(&[1.0, 2.0, 3.0, 4.0], 0.95), 4.0);
    }
}
