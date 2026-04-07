use alaz_core::Result;
use alaz_core::models::SignalWeight;
use sqlx::PgPool;

/// Input for upserting learned signal weights.
pub struct UpsertSignalWeight {
    pub query_type: String,
    pub fts: f32,
    pub dense: f32,
    pub raptor: f32,
    pub graph: f32,
    pub cue: f32,
    pub sample_size: i32,
}

pub struct SignalWeightRepo;

impl SignalWeightRepo {
    /// Get the latest learned weights for a query type.
    ///
    /// Returns `None` if no weights have been learned yet (falls back to defaults).
    pub async fn get(pool: &PgPool, query_type: &str) -> Result<Option<SignalWeight>> {
        let row = sqlx::query_as::<_, SignalWeight>(
            r#"
            SELECT id, query_type, fts, dense, raptor, graph, cue,
                   sample_size, created_at
            FROM signal_weights
            WHERE query_type = $1
            "#,
        )
        .bind(query_type)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Upsert learned weights for a query type.
    ///
    /// Called by the weight learning job after computing new weights from
    /// click-through data.
    pub async fn upsert(pool: &PgPool, input: &UpsertSignalWeight) -> Result<SignalWeight> {
        let query_type = &input.query_type;
        let fts = input.fts;
        let dense = input.dense;
        let raptor = input.raptor;
        let graph = input.graph;
        let cue = input.cue;
        let sample_size = input.sample_size;
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, SignalWeight>(
            r#"
            INSERT INTO signal_weights (id, query_type, fts, dense, raptor, graph, cue, sample_size)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (query_type) DO UPDATE SET
                fts = EXCLUDED.fts,
                dense = EXCLUDED.dense,
                raptor = EXCLUDED.raptor,
                graph = EXCLUDED.graph,
                cue = EXCLUDED.cue,
                sample_size = EXCLUDED.sample_size,
                created_at = now()
            RETURNING id, query_type, fts, dense, raptor, graph, cue,
                      sample_size, created_at
            "#,
        )
        .bind(&id)
        .bind(query_type)
        .bind(fts)
        .bind(dense)
        .bind(raptor)
        .bind(graph)
        .bind(cue)
        .bind(sample_size)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// List all learned weights (one per query type).
    pub async fn list(pool: &PgPool) -> Result<Vec<SignalWeight>> {
        let rows = sqlx::query_as::<_, SignalWeight>(
            r#"
            SELECT id, query_type, fts, dense, raptor, graph, cue,
                   sample_size, created_at
            FROM signal_weights
            ORDER BY query_type
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}
