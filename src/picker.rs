//! Fuzzy matching for the cluster picker.

/// How well `candidate` matches `query`, as a subsequence. None if it does
/// not match at all. Higher is better.
///
/// Scoring favours matches that start at a word boundary and run together,
/// so `prod` beats `p-r-o-d` scattered through a long name.
pub fn score(candidate: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = candidate.to_lowercase().chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().collect();

    let mut total = 0;
    let mut at = 0usize;
    let mut last: Option<usize> = None;

    for want in needle {
        let found = hay[at..].iter().position(|c| *c == want)? + at;
        let boundary = found == 0 || matches!(hay[found - 1], '-' | '_' | '.' | '/' | ' ');
        total += 1;
        if boundary {
            total += 6;
        }
        // A run of letters is the strongest signal, so it has to outweigh the
        // boundary bonus a hyphenated name would collect on every character.
        if last == Some(found.wrapping_sub(1)) {
            total += 10;
        }
        last = Some(found);
        at = found + 1;
    }
    // A shorter name carrying the same match is the better hit.
    Some(total - (candidate.len() as i32) / 8)
}

/// Candidates that match, best first. Ties keep their original order.
pub fn rank<'a>(candidates: &'a [String], query: &str) -> Vec<&'a str> {
    let mut hits: Vec<(i32, usize, &str)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| score(c, query).map(|s| (s, i, c.as_str())))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<String> {
        [
            "prod-us-east",
            "prod-eu-west",
            "staging-us",
            "dev-sandbox",
            "edge-k8s",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn an_empty_query_keeps_everything_in_order() {
        let all = items();
        assert_eq!(
            rank(&all, ""),
            all.iter().map(|s| s.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_prefix_narrows_to_the_obvious_ones() {
        let all = items();
        assert_eq!(rank(&all, "prod"), vec!["prod-us-east", "prod-eu-west"]);
    }

    #[test]
    fn matching_is_case_insensitive() {
        let all = items();
        assert_eq!(rank(&all, "PROD").len(), 2);
    }

    #[test]
    fn letters_can_be_scattered() {
        // p..e..w  ->  prod-eu-west
        let all = items();
        assert!(rank(&all, "pew").contains(&"prod-eu-west"));
    }

    #[test]
    fn a_query_that_matches_nothing_gives_nothing() {
        let all = items();
        assert!(rank(&all, "zzz").is_empty());
    }

    #[test]
    fn word_starts_outrank_letters_buried_mid_word() {
        let at_boundary = score("prod-us", "us").unwrap();
        let mid_word = score("produs", "us").unwrap();
        assert!(at_boundary > mid_word, "{at_boundary} vs {mid_word}");
    }

    #[test]
    fn a_run_of_letters_beats_a_scattered_match() {
        let candidates = vec!["p-r-o-d-x".to_string(), "prod".to_string()];
        assert_eq!(rank(&candidates, "prod")[0], "prod");
    }

    #[test]
    fn order_of_equals_is_stable() {
        let candidates = vec!["a-one".to_string(), "a-two".to_string()];
        assert_eq!(rank(&candidates, "a"), vec!["a-one", "a-two"]);
    }

    #[test]
    fn scoring_reports_no_match_rather_than_zero() {
        assert!(score("prod", "q").is_none());
        assert!(score("prod", "pd").is_some());
    }
}
