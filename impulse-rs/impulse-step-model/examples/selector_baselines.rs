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
//! Run: cargo run -p impulse-step-model --example selector_baselines -- runs.jsonl

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

#[derive(Debug, PartialEq)]
struct Report {
    name: String,
    tasks: usize,
    passes: usize,
    pass_rate: f64,
    cost_per_success: Option<f64>,
    p50_ms: f64,
    p95_ms: f64,
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[i]
}

fn report(name: &str, runs: &[&Run]) -> Report {
    let passes = runs.iter().filter(|r| r.pass).count();
    let cost: f64 = runs.iter().map(|r| r.cost_usd).sum();
    let mut lat: Vec<f64> = runs.iter().map(|r| r.latency_ms).collect();
    lat.sort_by(|a, b| a.total_cmp(b));
    Report {
        name: name.to_string(),
        tasks: runs.len(),
        passes,
        pass_rate: if runs.is_empty() { 0.0 } else { passes as f64 / runs.len() as f64 },
        cost_per_success: (passes > 0).then(|| cost / passes as f64),
        p50_ms: pct(&lat, 0.50),
        p95_ms: pct(&lat, 0.95),
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
            ra.total_cmp(&rb).then(b.1 .2.total_cmp(&a.1 .2)).then(b.0.cmp(a.0))
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
            match test.iter().find(|r| r.task_id == *t && r.policy == policy) {
                Some(r) => out.push(*r),
                None => {}
            }
        }
        out
    };

    let mut reports = vec![report("current selector", &pick("current"))];

    let global = best_fixed(train.iter().copied());
    match &global {
        Some(m) => reports.push(report(&format!("single best fixed ({m}, fitted on train)"), &pick(&format!("fixed:{m}")))),
        None => notes.push("no fixed-model train runs: single best fixed not reported".into()),
    }

    // Choose-at-start: one model per task, picked before the task starts from its
    // category, using the best fixed model for that category on train.
    let mut start = Vec::new();
    for t in &test_tasks {
        let cat = test.iter().find(|r| r.task_id == *t).map(|r| r.category.as_str()).unwrap_or("");
        let m = best_fixed(train.iter().copied().filter(|r| r.category == cat)).or_else(|| global.clone());
        if let Some(m) = m {
            if let Some(r) = test.iter().find(|r| r.task_id == *t && r.policy == format!("fixed:{m}")) {
                start.push(*r);
            }
        }
    }
    reports.push(report("choose-at-start (by category, fitted on train)", &start));

    for r in &reports {
        if r.tasks < test_tasks.len() {
            notes.push(format!("{}: {} of {} test tasks have a matching run", r.name, r.tasks, test_tasks.len()));
        }
    }
    (reports, notes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: selector_baselines <runs.jsonl>")?;
    let text = std::fs::read_to_string(path)?;
    let runs: Vec<Run> = text.lines().filter(|l| !l.trim().is_empty()).map(serde_json::from_str).collect::<Result<_, _>>()?;
    let (reports, notes) = evaluate(&runs);
    println!("{:<52}{:>7}{:>9}{:>14}{:>10}{:>10}", "policy (test split only)", "tasks", "pass", "$/success", "p50 ms", "p95 ms");
    for r in reports {
        let cps = r.cost_per_success.map(|c| format!("{c:.4}")).unwrap_or_else(|| "n/a".into());
        println!("{:<52}{:>7}{:>8.1}%{:>14}{:>10.0}{:>10.0}", r.name, r.tasks, 100.0 * r.pass_rate, cps, r.p50_ms, r.p95_ms);
    }
    for n in notes {
        println!("note: {n}");
    }
    println!("Cost includes escalation only where the recorded `current` runs include it.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(task: &str, split: &str, cat: &str, policy: &str, pass: bool, cost: f64, lat: f64) -> Run {
        Run { task_id: task.into(), split: split.into(), category: cat.into(), policy: policy.into(), pass, cost_usd: cost, latency_ms: lat }
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
        assert_eq!(best_fixed(d.iter().filter(|r| r.split == "train")), Some("sonnet".into()));
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

    #[test]
    fn percentiles() {
        assert_eq!(pct(&[1.0, 2.0, 3.0, 4.0], 0.5), 2.0);
        assert_eq!(pct(&[1.0, 2.0, 3.0, 4.0], 0.95), 4.0);
    }
}
