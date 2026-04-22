// Benchmarking module for performance analysis
// Provides tools to measure and analyze performance of various operations

use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub duration_ms: f64,
    pub iterations: u32,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub name: String,
    pub results: Vec<BenchmarkResult>,
}

/// Run a benchmark with a given number of iterations
pub fn run_benchmark<F>(name: &str, iterations: u32, mut f: F) -> BenchmarkResult
where
    F: FnMut(),
{
    let mut times: Vec<f64> = Vec::with_capacity(iterations as usize);

    for _ in 0..iterations {
        let start = Instant::now();
        f();
        let duration = start.elapsed();
        times.push(duration.as_secs_f64() * 1000.0);
    }

    let duration_ms = times.iter().sum::<f64>();
    let avg_ms = duration_ms / iterations as f64;
    let min_ms = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_ms = times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    BenchmarkResult {
        name: name.to_string(),
        duration_ms,
        iterations,
        avg_ms,
        min_ms,
        max_ms,
    }
}

/// Run an async benchmark with a given number of iterations
pub async fn run_async_benchmark<F, Fut>(name: &str, iterations: u32, mut f: F) -> BenchmarkResult
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut times: Vec<f64> = Vec::with_capacity(iterations as usize);

    for _ in 0..iterations {
        let start = Instant::now();
        f().await;
        let duration = start.elapsed();
        times.push(duration.as_secs_f64() * 1000.0);
    }

    let duration_ms = times.iter().sum::<f64>();
    let avg_ms = duration_ms / iterations as f64;
    let min_ms = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_ms = times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    BenchmarkResult {
        name: name.to_string(),
        duration_ms,
        iterations,
        avg_ms,
        min_ms,
        max_ms,
    }
}

/// Format a benchmark result for display
pub fn format_benchmark(result: &BenchmarkResult) -> String {
    format!(
        "{}: {} iterations in {:.2}ms (avg: {:.4}ms, min: {:.4}ms, max: {:.4}ms)",
        result.name,
        result.iterations,
        result.duration_ms,
        result.avg_ms,
        result.min_ms,
        result.max_ms
    )
}

/// Compare two benchmark results
pub fn compare_results(a: &BenchmarkResult, b: &BenchmarkResult) -> String {
    let speedup = b.avg_ms / a.avg_ms;
    let diff_ms = a.avg_ms - b.avg_ms;

    if speedup > 1.0 {
        format!(
            "{} is {:.2}x slower than {} ({:.4}ms difference)",
            b.name, speedup, a.name, diff_ms
        )
    } else {
        format!(
            "{} is {:.2}x faster than {} ({:.4}ms difference)",
            a.name,
            1.0 / speedup,
            a.name,
            diff_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark() {
        let result = run_benchmark("test", 1000, || {
            let _x = 1 + 1;
        });

        assert_eq!(result.name, "test");
        assert_eq!(result.iterations, 1000);
        assert!(result.avg_ms >= 0.0);
    }

    #[test]
    fn test_format_benchmark() {
        let result = BenchmarkResult {
            name: "test".to_string(),
            duration_ms: 100.0,
            iterations: 10,
            avg_ms: 10.0,
            min_ms: 9.5,
            max_ms: 10.5,
        };

        let formatted = format_benchmark(&result);
        assert!(formatted.contains("test"));
    }
}
