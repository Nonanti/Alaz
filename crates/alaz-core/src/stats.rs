/// Wilson score lower bound for ranking procedures by reliability.
///
/// Uses 95% confidence interval (z = 1.96). Returns the lower bound of the
/// confidence interval for the true success rate. This prevents items with
/// few observations (e.g. 1/1 = 100%) from outranking items with many
/// observations (e.g. 95/100 = 95%).
///
/// # Examples
/// ```
/// use alaz_core::wilson_score_lower;
///
/// // No data → None
/// assert_eq!(wilson_score_lower(0, 0), None);
///
/// // 1/1 → low confidence (~0.207)
/// let score = wilson_score_lower(1, 1).unwrap();
/// assert!(score < 0.25);
///
/// // 10/10 → higher confidence (~0.722)
/// let score = wilson_score_lower(10, 10).unwrap();
/// assert!(score > 0.7);
/// ```
pub fn wilson_score_lower(successes: i64, total: i64) -> Option<f64> {
    if total <= 0 {
        return None;
    }
    // Guard: successes cannot exceed total (data corruption / caller bug).
    // Clamp to avoid NaN from sqrt of a negative value.
    let successes = successes.min(total).max(0);

    let n = total as f64;
    let p = successes as f64 / n;
    let z = 1.96_f64; // 95% confidence
    let z2 = z * z; // 3.8416

    let numerator = p + z2 / (2.0 * n) - z * ((p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt());
    let denominator = 1.0 + z2 / n;

    Some((numerator / denominator).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_returns_none() {
        assert_eq!(wilson_score_lower(0, 0), None);
    }

    #[test]
    fn perfect_score_small_sample() {
        // 1/1 should give ~0.2065, much less than naive 1.0
        let score = wilson_score_lower(1, 1).unwrap();
        assert!((score - 0.2065).abs() < 0.01, "got {score}");
    }

    #[test]
    fn zero_success() {
        // 0/1 should give 0.0
        let score = wilson_score_lower(0, 1).unwrap();
        assert!(score.abs() < f64::EPSILON, "got {score}");
    }

    #[test]
    fn mixed_results() {
        // 2/3 should give ~0.2077
        let score = wilson_score_lower(2, 3).unwrap();
        assert!((score - 0.2077).abs() < 0.01, "got {score}");
    }

    #[test]
    fn high_sample_perfect() {
        // 10/10 should give ~0.7224
        let score = wilson_score_lower(10, 10).unwrap();
        assert!((score - 0.7224).abs() < 0.01, "got {score}");
    }

    #[test]
    fn large_sample() {
        // 95/100 should give ~0.893
        let score = wilson_score_lower(95, 100).unwrap();
        assert!((score - 0.893).abs() < 0.01, "got {score}");
    }

    #[test]
    fn monotonically_increases_with_evidence() {
        // Same 100% rate, more evidence → higher Wilson score
        let s1 = wilson_score_lower(1, 1).unwrap();
        let s5 = wilson_score_lower(5, 5).unwrap();
        let s10 = wilson_score_lower(10, 10).unwrap();
        let s50 = wilson_score_lower(50, 50).unwrap();
        assert!(s1 < s5, "1/1={s1} should be < 5/5={s5}");
        assert!(s5 < s10, "5/5={s5} should be < 10/10={s10}");
        assert!(s10 < s50, "10/10={s10} should be < 50/50={s50}");
    }

    #[test]
    fn high_evidence_beats_low_evidence() {
        // 95/100 should outrank 1/1 even though 1/1 has higher naive rate
        let low_evidence = wilson_score_lower(1, 1).unwrap();
        let high_evidence = wilson_score_lower(95, 100).unwrap();
        assert!(
            high_evidence > low_evidence,
            "95/100={high_evidence} should beat 1/1={low_evidence}"
        );
    }

    // ── Edge cases discovered in code review ─────────────────────────────────

    #[test]
    fn successes_greater_than_total_does_not_produce_nan() {
        // Corrupted data: successes > total must NOT return NaN — clamp to total.
        let score = wilson_score_lower(5, 3).unwrap();
        assert!(
            score.is_finite(),
            "wilson_score_lower(5,3) must be finite, got {score}"
        );
        // Clamping to 5→3 is equivalent to 3/3 (all successes), same as 3/3.
        let expected = wilson_score_lower(3, 3).unwrap();
        assert!(
            (score - expected).abs() < f64::EPSILON,
            "clamped 5/3 should equal 3/3={expected}, got {score}"
        );
    }

    #[test]
    fn negative_successes_clamps_to_zero() {
        // Negative successes (shouldn't happen, but defensive)
        let score = wilson_score_lower(-1, 3).unwrap();
        assert!(score.is_finite(), "got {score}");
        // -1 clamped to 0 ⇒ same as 0/3
        let expected = wilson_score_lower(0, 3).unwrap();
        assert!((score - expected).abs() < f64::EPSILON, "got {score}");
    }

    #[test]
    fn negative_total_returns_none() {
        assert_eq!(wilson_score_lower(0, -1), None);
        assert_eq!(wilson_score_lower(5, -5), None);
    }

    #[test]
    fn result_is_always_between_zero_and_one() {
        // Property: Wilson lower bound ∈ [0, 1] for all valid inputs
        for (s, t) in &[(0, 1), (1, 1), (5, 10), (10, 10), (50, 100), (99, 100)] {
            let score = wilson_score_lower(*s, *t).unwrap();
            assert!(
                (0.0..=1.0).contains(&score),
                "wilson_score_lower({s},{t})={score} should be in [0,1]"
            );
        }
    }

    #[test]
    fn tighter_tolerance_spot_checks() {
        // Deterministic formula — use ≤0.001 tolerance for precision
        let cases = [
            (1, 1, 0.2065_f64),
            (0, 1, 0.0),
            (2, 3, 0.2077),
            (10, 10, 0.7224),
            (95, 100, 0.8882),
        ];
        for (s, t, expected) in cases {
            let score = wilson_score_lower(s, t).unwrap();
            assert!(
                (score - expected).abs() < 0.001,
                "wilson_score_lower({s},{t}): expected ~{expected}, got {score}"
            );
        }
    }

    #[test]
    fn zero_total_but_nonzero_successes_returns_none() {
        // total=0 always returns None regardless of successes
        assert_eq!(wilson_score_lower(5, 0), None);
    }
}
