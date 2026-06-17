// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Predictive (fuzzy) ranking of key names.
//!
//! This is the single source of truth for how a query is matched against the
//! stored key names, shared so the daemon (and any other front-end linking
//! `libsalus`) ranks identically.

use nucleo_matcher::{
    Config, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};

/// Fuzzy-rank `candidates` against `query`, best match first.
///
/// An empty `query` returns every candidate sorted alphabetically (a stable,
/// predictable "list all" order). A non-empty `query` is matched as a fuzzy
/// subsequence (case-insensitive); non-matching candidates are dropped and the
/// rest are ordered by match score, highest first. When `limit` is `Some(n)`,
/// at most `n` results are returned.
#[must_use]
pub fn fuzzy_rank(query: &str, mut candidates: Vec<String>, limit: Option<usize>) -> Vec<String> {
    if query.is_empty() {
        candidates.sort();
        if let Some(n) = limit {
            candidates.truncate(n);
        }
        return candidates;
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    // `match_list` drops non-matches and returns the remainder sorted by score,
    // highest first.
    let ranked = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart)
        .match_list(candidates, &mut matcher);
    let mut out: Vec<String> = ranked.into_iter().map(|(name, _score)| name).collect();
    if let Some(n) = limit {
        out.truncate(n);
    }
    out
}

#[cfg(test)]
mod test {
    use super::fuzzy_rank;

    fn keys() -> Vec<String> {
        vec![
            "aws-prod-key".to_string(),
            "aws-staging".to_string(),
            "github-token".to_string(),
            "gitlab-hook".to_string(),
        ]
    }

    #[test]
    fn empty_query_lists_all_sorted() {
        let ranked = fuzzy_rank("", keys(), None);
        assert_eq!(
            ranked,
            vec![
                "aws-prod-key".to_string(),
                "aws-staging".to_string(),
                "github-token".to_string(),
                "gitlab-hook".to_string(),
            ]
        );
    }

    #[test]
    fn contiguous_match_ranks_above_scattered_subsequence() {
        // "abc" is a contiguous prefix of "abc-key" but only a scattered
        // subsequence (a..b..c) of "a-b-c-key"; the contiguous one must rank
        // first.
        let candidates = vec!["a-b-c-key".to_string(), "abc-key".to_string()];
        let ranked = fuzzy_rank("abc", candidates, None);
        assert_eq!(ranked.first().map(String::as_str), Some("abc-key"));
        assert!(ranked.iter().any(|k| k == "a-b-c-key"));
    }

    #[test]
    fn case_insensitive_match() {
        let ranked = fuzzy_rank("AWS", keys(), None);
        assert!(ranked.iter().any(|k| k == "aws-prod-key"));
        assert!(ranked.iter().any(|k| k == "aws-staging"));
    }

    #[test]
    fn limit_truncates_results() {
        let ranked = fuzzy_rank("aws", keys(), Some(1));
        assert_eq!(ranked.len(), 1);
    }

    #[test]
    fn empty_query_with_limit_truncates_sorted() {
        // The "list all" branch must honor the limit too, returning the first
        // `n` candidates in sorted order.
        let ranked = fuzzy_rank("", keys(), Some(2));
        assert_eq!(
            ranked,
            vec!["aws-prod-key".to_string(), "aws-staging".to_string()]
        );
    }

    #[test]
    fn no_match_returns_empty() {
        let ranked = fuzzy_rank("zzzznope", keys(), None);
        assert!(ranked.is_empty());
    }
}
