-- Switch FTS from English to simple (language-agnostic)
-- Enables Turkish and other non-English content to be found via FTS.
-- 'simple' does no stemming but tokenizes all languages correctly.
-- Stemming loss is acceptable because vector search handles semantic matching.

-- knowledge_items
ALTER TABLE knowledge_items DROP COLUMN search_vector;
ALTER TABLE knowledge_items ADD COLUMN search_vector TSVECTOR GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(title, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(description, '')), 'B') ||
    setweight(to_tsvector('simple', coalesce(content, '')), 'C')
) STORED;
CREATE INDEX IF NOT EXISTS idx_knowledge_items_search_vector ON knowledge_items USING GIN(search_vector);

-- episodes
ALTER TABLE episodes DROP COLUMN search_vector;
ALTER TABLE episodes ADD COLUMN search_vector TSVECTOR GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(title, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(content, '')), 'B')
) STORED;
CREATE INDEX IF NOT EXISTS idx_episodes_search_vector ON episodes USING GIN(search_vector);

-- procedures
ALTER TABLE procedures DROP COLUMN search_vector;
ALTER TABLE procedures ADD COLUMN search_vector TSVECTOR GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(title, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(content, '')), 'B')
) STORED;
CREATE INDEX IF NOT EXISTS idx_procedures_search_vector ON procedures USING GIN(search_vector);

-- reflections
ALTER TABLE reflections DROP COLUMN search_vector;
ALTER TABLE reflections ADD COLUMN search_vector TSVECTOR GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(what_worked, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(what_failed, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(lessons_learned, '')), 'B')
) STORED;
CREATE INDEX IF NOT EXISTS idx_reflections_search_vector ON reflections USING GIN(search_vector);
