use chrono::{DateTime, Utc};

/// Compute a relevance score for a graph entity.
///
/// Combines edge weight, recency of use (half-life ~23 days), usage frequency,
/// and maturity (age capped at 2x boost after 30 days).
///
/// Formula:
///   weight * recency * usage * maturity
///
/// Where:
///   - recency = exp(-0.693 / 23.0 * days_since_used)   (half-life ~23 days)
///   - usage   = ln(1 + usage_count)
///   - maturity = min(1 + age_days / 30, 2.0)
pub fn relevance_score(
    weight: f64,
    last_used: DateTime<Utc>,
    usage_count: i32,
    created_at: DateTime<Utc>,
) -> f64 {
    let now = Utc::now();

    let days_since_used = (now - last_used).num_seconds() as f64 / 86400.0;
    let recency = (-0.693_f64 / 23.0 * days_since_used).exp();

    let usage = (1.0 + usage_count.max(0) as f64).ln().max(0.1);

    let age_days = (now - created_at).num_seconds() as f64 / 86400.0;
    let maturity = (1.0 + age_days / 30.0).min(2.0);

    weight * recency * usage * maturity
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_fresh_high_usage() {
        let now = Utc::now();
        let score = relevance_score(1.0, now, 10, now - Duration::days(60));
        // High usage (ln(11) ~= 2.4), full recency (1.0), max maturity (2.0)
        assert!(score > 4.0, "expected high score, got {score}");
    }

    #[test]
    fn test_stale_low_usage() {
        let now = Utc::now();
        let score = relevance_score(0.5, now - Duration::days(90), 1, now - Duration::days(90));
        // Low weight, decayed recency, low usage
        assert!(score < 1.0, "expected low score, got {score}");
    }

    #[test]
    fn test_zero_usage_has_minimum_score() {
        let now = Utc::now();
        let score = relevance_score(1.0, now, 0, now);
        // usage = max(ln(1), 0.1) = 0.1, so score > 0
        assert!(
            score > 0.0,
            "expected non-zero score for unused items, got {score}"
        );
        assert!(
            score < 0.5,
            "expected low score for unused items, got {score}"
        );
    }

    #[test]
    fn test_young_item_low_maturity() {
        let now = Utc::now();
        let score = relevance_score(1.0, now, 5, now);
        // maturity ~= 1.0 (age 0 days), usage = ln(6) ~= 1.79, recency = 1.0
        let expected_approx = 1.0 * 1.0 * (1.0 + 5.0_f64).ln() * 1.0;
        assert!(
            (score - expected_approx).abs() < 0.01,
            "expected ~{expected_approx}, got {score}"
        );
    }

    // --- Boundary tests ---

    #[test]
    fn test_zero_weight_gives_zero_score() {
        let now = Utc::now();
        let score = relevance_score(0.0, now, 100, now - Duration::days(60));
        assert!(
            score.abs() < 0.001,
            "zero weight should yield zero score, got {score}"
        );
    }

    #[test]
    fn test_future_last_used_date() {
        let now = Utc::now();
        let future = now + Duration::days(7);
        // Future last_used → negative days_since_used → recency > 1.0 (exp of positive)
        let score = relevance_score(1.0, future, 5, now);
        assert!(
            score > 0.0,
            "future last_used should still give positive score, got {score}"
        );
        // recency = exp(positive) > 1.0, so score should be higher than normal
        let normal = relevance_score(1.0, now, 5, now);
        assert!(
            score > normal,
            "future last_used should boost recency: {score} vs {normal}"
        );
    }

    #[test]
    fn test_very_old_item_maturity_capped() {
        let now = Utc::now();
        let score_365 = relevance_score(1.0, now, 5, now - Duration::days(365));
        let score_60 = relevance_score(1.0, now, 5, now - Duration::days(60));
        // Both should have maturity capped at 2.0 (threshold is 30 days for cap)
        assert!(
            (score_365 - score_60).abs() < 0.001,
            "maturity should be capped at 2.0 for both: 365d={score_365}, 60d={score_60}"
        );
    }

    #[test]
    fn test_negative_usage_count() {
        let now = Utc::now();
        let score = relevance_score(1.0, now, -5, now);
        assert!(score.is_finite(), "got {score}");
        // -5 clamped to 0 => same as usage_count=0
        let expected = relevance_score(1.0, now, 0, now);
        assert!(
            (score - expected).abs() < f64::EPSILON,
            "clamped -5 should equal 0: got {score}, expected {expected}"
        );
    }
}
