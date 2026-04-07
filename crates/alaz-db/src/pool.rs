use std::collections::HashSet;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

pub async fn create_pool(database_url: &str) -> alaz_core::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// All migrations in order. Each entry is (version_key, sql_content).
const MIGRATIONS: &[(&str, &str)] = &[
    ("001", include_str!("migrations/001_initial.sql")),
    ("002", include_str!("migrations/002_vault.sql")),
    ("003", include_str!("migrations/003_dedup_indexes.sql")),
    ("004", include_str!("migrations/004_memory_decay.sql")),
    ("005", include_str!("migrations/005_search_feedback.sql")),
    ("006", include_str!("migrations/006_enhanced_features.sql")),
    ("007", include_str!("migrations/007_source_tracking.sql")),
    ("008", include_str!("migrations/008_simple_fts.sql")),
    ("009", include_str!("migrations/009_wilson_score.sql")),
    ("010", include_str!("migrations/010_episode_enrichment.sql")),
    ("011", include_str!("migrations/011_pattern_usage.sql")),
    ("012", include_str!("migrations/012_signal_weights.sql")),
    (
        "013",
        include_str!("migrations/013_search_explanations.sql"),
    ),
    ("014", include_str!("migrations/014_git_activity.sql")),
    ("015", include_str!("migrations/015_code_symbols.sql")),
    ("016", include_str!("migrations/016_spaced_repetition.sql")),
    ("017", include_str!("migrations/017_context_tracking.sql")),
    ("018", include_str!("migrations/018_learning_analytics.sql")),
];

/// Full migration file names for display purposes.
const MIGRATION_NAMES: &[(&str, &str)] = &[
    ("001", "001_initial.sql"),
    ("002", "002_vault.sql"),
    ("003", "003_dedup_indexes.sql"),
    ("004", "004_memory_decay.sql"),
    ("005", "005_search_feedback.sql"),
    ("006", "006_enhanced_features.sql"),
    ("007", "007_source_tracking.sql"),
    ("008", "008_simple_fts.sql"),
    ("009", "009_wilson_score.sql"),
    ("010", "010_episode_enrichment.sql"),
    ("011", "011_pattern_usage.sql"),
    ("012", "012_signal_weights.sql"),
    ("013", "013_search_explanations.sql"),
    ("014", "014_git_activity.sql"),
    ("015", "015_code_symbols.sql"),
    ("016", "016_spaced_repetition.sql"),
    ("017", "017_context_tracking.sql"),
    ("018", "018_learning_analytics.sql"),
];

/// Migration status for a single migration file.
#[derive(Debug)]
pub struct MigrationInfo {
    pub version: String,
    pub name: String,
    pub applied: bool,
}

/// Ensure the `_alaz_migrations` tracking table exists.
async fn ensure_tracking_table(pool: &PgPool) -> alaz_core::Result<()> {
    let sql = include_str!("migrations/000_migration_tracking.sql");
    sqlx::raw_sql(sql).execute(pool).await?;
    Ok(())
}

/// Get the set of already-applied migration versions.
async fn get_applied_versions(pool: &PgPool) -> alaz_core::Result<HashSet<String>> {
    let rows = sqlx::query("SELECT version FROM _alaz_migrations")
        .fetch_all(pool)
        .await?;
    let versions: HashSet<String> = rows.iter().map(|r| r.get("version")).collect();
    Ok(versions)
}

/// Detect if this is a pre-tracking database (tables exist but no tracking table was populated).
/// We check for the `knowledge_items` table as a sentinel — if it exists, all prior migrations
/// were already applied before tracking was added.
async fn is_pre_tracking_database(pool: &PgPool) -> alaz_core::Result<bool> {
    let row = sqlx::query(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = 'knowledge_items'
        ) AS exists",
    )
    .fetch_one(pool)
    .await?;
    Ok(row.get::<bool, _>("exists"))
}

/// Backfill all existing migrations as applied (for databases that predate tracking).
async fn backfill_applied_versions(pool: &PgPool) -> alaz_core::Result<()> {
    for (version, _) in MIGRATIONS {
        sqlx::query("INSERT INTO _alaz_migrations (version) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(version)
            .execute(pool)
            .await?;
    }
    tracing::info!(
        "backfilled {} migrations as already applied (pre-tracking database)",
        MIGRATIONS.len()
    );
    Ok(())
}

/// Run pending database migrations, returning the count of newly applied migrations.
pub async fn run_migrations(pool: &PgPool) -> alaz_core::Result<usize> {
    // Step 1: Ensure tracking table exists
    ensure_tracking_table(pool).await?;

    // Step 2: Handle pre-tracking databases (tables exist but no tracking records)
    let applied = get_applied_versions(pool).await?;
    if applied.is_empty() && is_pre_tracking_database(pool).await? {
        backfill_applied_versions(pool).await?;
        tracing::info!("migrations completed (0 new — all backfilled)");
        return Ok(0);
    }

    // Step 3: Run pending migrations
    let mut count = 0usize;
    for (version, sql) in MIGRATIONS {
        if applied.contains(*version) {
            continue;
        }

        tracing::info!(version, "applying migration");
        sqlx::raw_sql(sql).execute(pool).await?;

        sqlx::query("INSERT INTO _alaz_migrations (version) VALUES ($1)")
            .bind(version)
            .execute(pool)
            .await?;

        count += 1;
    }

    tracing::info!(applied = count, "migrations completed");
    Ok(count)
}

/// Return migration status (applied/pending) without executing anything.
pub async fn migration_status(pool: &PgPool) -> alaz_core::Result<Vec<MigrationInfo>> {
    ensure_tracking_table(pool).await?;
    let applied = get_applied_versions(pool).await?;

    let mut result = Vec::with_capacity(MIGRATION_NAMES.len());
    for (version, name) in MIGRATION_NAMES {
        result.push(MigrationInfo {
            version: version.to_string(),
            name: name.to_string(),
            applied: applied.contains(*version),
        });
    }
    Ok(result)
}

/// Dry-run: return versions that would be applied, without executing them.
pub async fn migrations_pending(pool: &PgPool) -> alaz_core::Result<Vec<MigrationInfo>> {
    ensure_tracking_table(pool).await?;
    let applied = get_applied_versions(pool).await?;

    let mut pending = Vec::new();
    for (version, name) in MIGRATION_NAMES {
        if !applied.contains(*version) {
            pending.push(MigrationInfo {
                version: version.to_string(),
                name: name.to_string(),
                applied: false,
            });
        }
    }
    Ok(pending)
}
