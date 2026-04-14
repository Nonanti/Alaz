-- GIN index on search_queries.result_ids for efficient array membership checks
CREATE INDEX IF NOT EXISTS idx_search_queries_result_ids ON search_queries USING GIN (result_ids);
