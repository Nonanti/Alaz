ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS times_used BIGINT NOT NULL DEFAULT 0;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS times_success BIGINT NOT NULL DEFAULT 0;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS pattern_score DOUBLE PRECISION GENERATED ALWAYS AS (
  CASE WHEN times_used > 0 THEN
    ((times_success::double precision / times_used::double precision + 1.9208 / times_used::double precision) - 
     1.96 * sqrt((times_success::double precision / times_used::double precision * (1.0 - times_success::double precision / times_used::double precision)) / times_used::double precision + 0.9604 / (times_used::double precision * times_used::double precision))) / 
    (1.0 + 3.8416 / times_used::double precision)
  ELSE NULL END
) STORED;
