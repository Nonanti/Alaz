-- Replace naive success_rate (successes/total) with Wilson score lower bound.
--
-- Wilson score lower bound provides a statistically reliable ranking for
-- procedures with small sample sizes. It computes the lower bound of the
-- 95% confidence interval for the true success rate.
--
-- Formula: (p̂ + z²/2n - z√(p̂(1-p̂)/n + z²/4n²)) / (1 + z²/n)
-- where p̂ = success/total, n = total attempts, z = 1.96 (95% confidence)
--
-- Key constants: z = 1.96, z² = 3.8416, z²/2 = 1.9208, z²/4 = 0.9604
--
-- Examples (naive → Wilson):
--   1/1 (100%) → 0.207  (insufficient data, penalized)
--   5/5 (100%) → 0.566  (gaining confidence)
--  10/10(100%) → 0.722  (high confidence)
--  95/100(95%) → 0.893  (very high confidence)
--   2/3 (67%)  → 0.208  (too few tries to be sure)

ALTER TABLE procedures DROP COLUMN success_rate;
ALTER TABLE procedures ADD COLUMN success_rate DOUBLE PRECISION GENERATED ALWAYS AS (
    CASE WHEN times_used > 0 THEN
        (
            (times_success::DOUBLE PRECISION / times_used::DOUBLE PRECISION)
            + 1.9208 / times_used::DOUBLE PRECISION
            - 1.96 * sqrt(
                (times_success::DOUBLE PRECISION / times_used::DOUBLE PRECISION)
                * (1.0 - times_success::DOUBLE PRECISION / times_used::DOUBLE PRECISION)
                / times_used::DOUBLE PRECISION
                + 0.9604 / (times_used::DOUBLE PRECISION * times_used::DOUBLE PRECISION)
            )
        ) / (1.0 + 3.8416 / times_used::DOUBLE PRECISION)
    ELSE NULL
    END
) STORED;
