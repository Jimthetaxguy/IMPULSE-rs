use crate::retrieval::types::SearchResult;

const MAX_EDITS: usize = 2;

pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.len();
    let len2 = s2.len();

    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];

    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate() {
        *cell = j;
    }

    let s1_bytes = s1.as_bytes();
    let s2_bytes = s2.as_bytes();

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1_bytes[i - 1] == s2_bytes[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[len1][len2]
}

pub fn is_fuzzy_match(query: &str, target: &str) -> bool {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if target_lower.contains(&query_lower) {
        return false;
    }

    let distance = levenshtein_distance(&query_lower, &target_lower);
    distance <= MAX_EDITS
}

pub fn fuzzy_score(query: &str, target: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if target_lower.contains(&query_lower) {
        return 1.0;
    }

    let distance = levenshtein_distance(&query_lower, &target_lower);
    if distance > MAX_EDITS {
        return 0.0;
    }

    let max_len = query_lower.len().max(target_lower.len());
    if max_len == 0 {
        return 1.0;
    }

    let similarity = 1.0 - (distance as f64 / max_len as f64);
    similarity * 0.99
}

pub fn boost_exact_matches(results: Vec<SearchResult>, query: &str) -> Vec<SearchResult> {
    let query_lower = query.to_lowercase();

    let mut exact: Vec<SearchResult> = Vec::new();
    let mut fuzzy: Vec<SearchResult> = Vec::new();

    for mut result in results {
        let title_match = result.title.to_lowercase().contains(&query_lower);
        let snippet_match = result.snippet.to_lowercase().contains(&query_lower);

        if title_match {
            result.score += 0.02;
            exact.push(result);
        } else if snippet_match {
            result.score += 0.01;
            exact.push(result);
        } else {
            fuzzy.push(result);
        }
    }

    exact.into_iter().chain(fuzzy).collect()
}

pub fn apply_fuzzy_filter(
    results: Vec<SearchResult>,
    query: &str,
    fuzzy_enabled: bool,
) -> Vec<SearchResult> {
    if !fuzzy_enabled {
        return results;
    }

    let mut scored_results: Vec<(SearchResult, f64)> = results
        .into_iter()
        .map(|r| {
            let title_score = fuzzy_score(query, &r.title);
            let snippet_score = fuzzy_score(query, &r.snippet);
            let max_score = title_score.max(snippet_score);
            (r, max_score)
        })
        .collect();

    scored_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let has_exact = scored_results
        .iter()
        .any(|(_, score)| (*score - 1.0).abs() < 0.001);

    if has_exact {
        scored_results
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(mut r, fuzzy_score)| {
                if (fuzzy_score - 1.0).abs() < 0.001 {
                    r.score += 0.01;
                }
                r
            })
            .collect()
    } else {
        scored_results
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(r, _)| r)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
    }

    #[test]
    fn test_levenshtein_one_insertion() {
        assert_eq!(levenshtein_distance("hello", "helo"), 1);
    }

    #[test]
    fn test_levenshtein_one_deletion() {
        assert_eq!(levenshtein_distance("hello", "hell"), 1);
    }

    #[test]
    fn test_levenshtein_one_substitution() {
        assert_eq!(levenshtein_distance("hello", "hallo"), 1);
    }

    #[test]
    fn test_levenshtein_two_edits() {
        assert_eq!(levenshtein_distance("hello", "hola"), 3);
    }

    #[test]
    fn test_levenshtein_empty() {
        assert_eq!(levenshtein_distance("", "hello"), 5);
        assert_eq!(levenshtein_distance("hello", ""), 5);
        assert_eq!(levenshtein_distance("", ""), 0);
    }

    #[test]
    fn test_is_fuzzy_match_within_threshold() {
        assert!(is_fuzzy_match("hello", "hallo"));
        assert!(is_fuzzy_match("hello", "holla"));
        assert!(is_fuzzy_match("test", "text"));
    }

    #[test]
    fn test_is_fuzzy_match_exact_contains() {
        assert!(!is_fuzzy_match("hello", "hello world"));
        assert!(!is_fuzzy_match("world", "hello world"));
    }

    #[test]
    fn test_is_fuzzy_match_over_threshold() {
        assert!(!is_fuzzy_match("hello", "goodbye"));
        assert!(!is_fuzzy_match("test", "completely different"));
    }

    #[test]
    fn test_fuzzy_score_exact_contains() {
        let score = fuzzy_score("hello", "hello world");
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_fuzzy_score_within_threshold() {
        let score = fuzzy_score("hello", "hallo");
        assert!(score > 0.0);
        assert!(score < 1.0);
    }

    #[test]
    fn test_fuzzy_score_over_threshold() {
        let score = fuzzy_score("hello", "goodbye");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_boost_exact_matches() {
        let results = vec![
            SearchResult {
                source: "test".to_string(),
                id: "1".to_string(),
                title: "test query".to_string(),
                snippet: "some text".to_string(),
                score: 0.5,
            },
            SearchResult {
                source: "test".to_string(),
                id: "2".to_string(),
                title: "other".to_string(),
                snippet: "test text".to_string(),
                score: 0.5,
            },
            SearchResult {
                source: "test".to_string(),
                id: "3".to_string(),
                title: "unrelated".to_string(),
                snippet: "content".to_string(),
                score: 0.5,
            },
        ];

        let boosted = boost_exact_matches(results, "test");

        assert!(boosted[0].score > boosted[1].score);
        assert!(boosted[1].score > boosted[2].score);
    }

    #[test]
    fn test_apply_fuzzy_filter_disabled() {
        let results = vec![SearchResult {
            source: "test".to_string(),
            id: "1".to_string(),
            title: "hello".to_string(),
            snippet: "world".to_string(),
            score: 0.5,
        }];

        let filtered = apply_fuzzy_filter(results, "hello", false);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_apply_fuzzy_filter_enabled_filters_far() {
        let results = vec![
            SearchResult {
                source: "test".to_string(),
                id: "1".to_string(),
                title: "hello".to_string(),
                snippet: "world".to_string(),
                score: 0.5,
            },
            SearchResult {
                source: "test".to_string(),
                id: "2".to_string(),
                title: "completely different".to_string(),
                snippet: "text".to_string(),
                score: 0.5,
            },
        ];

        let filtered = apply_fuzzy_filter(results, "hello", true);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "1");
    }
}
