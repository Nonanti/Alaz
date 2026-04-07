//! Memory decay scoring.
//!
//! Applies a time-based exponential decay and usage-frequency boost to search
//! scores, modeling how memory strength fades over time but is reinforced by
//! repeated access.

use chrono::{DateTime, Utc};

/// Apply memory decay to a search score.
///
/// - `score`: the raw fused score from RRF
/// - `last_accessed`: when the entity was last accessed (None defaults to 30 days ago)
/// - `access_count`: how many times the entity has been accessed
///
/// Recency uses exponential decay with a 30-day half-life.
/// Usage applies a logarithmic boost based on access count.
pub fn apply_decay(score: f64, last_accessed: Option<DateTime<Utc>>, access_count: i32) -> f64 {
    let days = last_accessed
        .map(|la| (Utc::now() - la).num_seconds() as f64 / 86400.0)
        .unwrap_or(30.0);

    // Exponential decay with 30-day half-life: exp(-ln(2)/30 * days)
    let recency = (-0.693_f64 / 30.0 * days).exp();

    // Logarithmic usage boost: 1 + ln(1 + count) * 0.1
    let usage = 1.0 + (1.0 + access_count as f64).ln() * 0.1;

    score * recency * usage
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_recent_access_high_score() {
        let now = Utc::now();
        let score = apply_decay(1.0, Some(now), 0);
        // Just accessed, should be close to 1.0 * 1.0 * (1 + ln(1)*0.1)
        // = 1.0 * ~1.0 * 1.0 = ~1.0
        assert!(
            score > 0.9,
            "Recent access should preserve most of the score, got {score}"
        );
    }

    #[test]
    fn test_old_access_low_score() {
        let old = Utc::now() - Duration::days(90);
        let score = apply_decay(1.0, Some(old), 0);
        // 90 days = 3 half-lives, should be ~1/8 of original
        assert!(
            score < 0.2,
            "90-day-old access should have significant decay, got {score}"
        );
    }

    #[test]
    fn test_half_life_at_30_days() {
        let thirty_days_ago = Utc::now() - Duration::days(30);
        let score = apply_decay(1.0, Some(thirty_days_ago), 0);
        // At exactly 30 days, recency should be ~0.5, usage is 1.0
        assert!(
            (score - 0.5).abs() < 0.05,
            "Score at 30 days should be approximately 0.5, got {score}"
        );
    }

    #[test]
    fn test_no_last_accessed_defaults_to_30_days() {
        let with_none = apply_decay(1.0, None, 0);
        let thirty_days_ago = Utc::now() - Duration::days(30);
        let with_30d = apply_decay(1.0, Some(thirty_days_ago), 0);
        assert!(
            (with_none - with_30d).abs() < 0.01,
            "None should default to ~30 days, got none={with_none} vs 30d={with_30d}"
        );
    }

    #[test]
    fn test_high_usage_boosts_score() {
        let now = Utc::now();
        let low_usage = apply_decay(1.0, Some(now), 1);
        let high_usage = apply_decay(1.0, Some(now), 100);
        assert!(
            high_usage > low_usage,
            "Higher usage should boost score: low={low_usage}, high={high_usage}"
        );
    }

    #[test]
    fn test_usage_boost_is_logarithmic() {
        let now = Utc::now();

        // Test sublinearity: equal-sized intervals should produce
        // diminishing absolute returns.
        // Interval 1: count 0 -> 100 (100 units)
        // Interval 2: count 100 -> 200 (100 units)
        // Interval 3: count 200 -> 300 (100 units)
        let s0 = apply_decay(1.0, Some(now), 0);
        let s100 = apply_decay(1.0, Some(now), 100);
        let s200 = apply_decay(1.0, Some(now), 200);
        let s300 = apply_decay(1.0, Some(now), 300);

        let diff_0_100 = s100 - s0;
        let diff_100_200 = s200 - s100;
        let diff_200_300 = s300 - s200;

        assert!(
            diff_0_100 > diff_100_200,
            "Usage boost should be sublinear: diff_0_100={diff_0_100}, diff_100_200={diff_100_200}"
        );
        assert!(
            diff_100_200 > diff_200_300,
            "Usage boost should be sublinear: diff_100_200={diff_100_200}, diff_200_300={diff_200_300}"
        );

        // Also verify the boost is bounded and reasonable
        let s10000 = apply_decay(1.0, Some(now), 10000);
        assert!(
            s10000 < s0 * 2.0,
            "Even 10000 accesses should not more than double the score"
        );
    }

    #[test]
    fn test_zero_score_stays_zero() {
        let now = Utc::now();
        let score = apply_decay(0.0, Some(now), 100);
        assert!(
            score.abs() < 1e-10,
            "Zero score should remain zero regardless of decay/usage"
        );
    }

    #[test]
    fn test_none_last_accessed_still_works() {
        // Should not panic and should return a reasonable value
        let score = apply_decay(1.0, None, 5);
        assert!(score > 0.0, "Should produce a positive score");
        assert!(
            score < 1.0,
            "With None (defaults to 30d), score should be decayed"
        );
    }

    #[test]
    fn test_very_high_access_count() {
        let now = Utc::now();
        let score = apply_decay(1.0, Some(now), 1_000_000);
        // Even with a million accesses, logarithmic boost shouldn't explode
        // usage = 1.0 + ln(1 + 1_000_000) * 0.1 ≈ 1 + 13.8 * 0.1 ≈ 2.38
        assert!(
            score < 3.0,
            "Million accesses should not make score explode, got {score}"
        );
        assert!(
            score > 1.0,
            "High usage should still boost above base, got {score}"
        );
    }

    #[test]
    fn test_zero_base_score() {
        let now = Utc::now();
        let score = apply_decay(0.0, Some(now), 1000);
        assert!(
            score.abs() < 1e-10,
            "Zero base score should stay zero even with high access count"
        );
    }

    #[test]
    fn test_negative_score_preserved_direction() {
        // Edge case: negative input score
        let now = Utc::now();
        let score = apply_decay(-1.0, Some(now), 0);
        assert!(
            score < 0.0,
            "Negative input should remain negative, got {score}"
        );
    }
}
