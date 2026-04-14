mod generate;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use tracing::info;

#[derive(Parser)]
#[command(name = "alaz", about = "Alaz -- AI Knowledge System", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP/MCP server
    Serve,
    /// Run database migrations
    Migrate {
        /// Show migration status (applied/pending) without running
        #[arg(long)]
        status: bool,
        /// Show what would be applied without running
        #[arg(long)]
        dry_run: bool,
    },
    /// Session lifecycle hooks
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Rebuild RAPTOR hierarchical clustering tree
    RaptorRebuild {
        /// Project name to rebuild (all projects if omitted)
        #[arg(long)]
        project: Option<String>,
    },
    /// Manage owners
    Owner {
        #[command(subcommand)]
        action: OwnerAction,
    },
    /// Manage devices (trusted device authentication)
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },
    /// Manage API keys
    Apikey {
        #[command(subcommand)]
        action: ApikeyAction,
    },
    /// Manage vault secrets
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },
    /// View audit logs
    Audit {
        /// Filter by owner ID
        #[arg(long)]
        owner: Option<String>,
        /// Filter by event type
        #[arg(long)]
        event: Option<String>,
        /// Maximum number of entries
        #[arg(long, default_value = "50")]
        limit: i64,
    },
    /// Generate a CLAUDE.md file from Alaz knowledge base
    GenerateClaudeMd {
        /// Project name (or auto-detect from cwd)
        #[arg(long)]
        project: Option<String>,
        /// Output file path (default: CLAUDE.md in current dir)
        #[arg(long)]
        output: Option<String>,
        /// Max output size in characters (default: 16000)
        #[arg(long, default_value = "16000")]
        max_chars: usize,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Hook: session started — outputs context JSON for the AI assistant
    Start {
        /// Project name / working directory
        #[arg(long)]
        project: Option<String>,
        /// Remote Alaz server URL (e.g. https://alaz.example.com) — uses HTTP API instead of direct DB
        #[arg(long)]
        url: Option<String>,
        /// API key for remote server authentication
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Hook: session stopped — triggers learning pipeline
    Stop {
        /// Session ID to learn from
        #[arg(long)]
        session_id: Option<String>,
        /// The transcript to process
        #[arg(long)]
        transcript: Option<String>,
        /// Project name
        #[arg(long)]
        project: Option<String>,
        /// Remote Alaz server URL (e.g. https://alaz.example.com) — uses HTTP API instead of direct DB
        #[arg(long)]
        url: Option<String>,
        /// API key for remote server authentication
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Hook: build compact restore context for resuming a session
    Compact {
        /// Session ID to restore context for
        #[arg(long)]
        session_id: String,
        /// Project name
        #[arg(long)]
        project: Option<String>,
    },
    /// Hook: proactive context injection (PostToolUse hook)
    Context {
        /// Remote Alaz server URL (e.g. http://localhost:3456)
        #[arg(long, default_value = "http://localhost:3456")]
        url: String,
    },
}

#[derive(Subcommand)]
enum OwnerAction {
    /// Create a new owner
    Create {
        /// Username
        #[arg(long)]
        username: String,
        /// Password (will be hashed)
        #[arg(long)]
        password: String,
    },
    /// List all owners
    List,
}

#[derive(Subcommand)]
enum DeviceAction {
    /// List registered devices
    List {
        /// Filter by owner ID
        #[arg(long)]
        owner: Option<String>,
    },
    /// Approve (trust) a device
    Approve {
        /// Device ID
        id: String,
    },
    /// Revoke trust from a device
    Revoke {
        /// Device ID
        id: String,
    },
    /// Delete a device registration
    Delete {
        /// Device ID
        id: String,
    },
}

#[derive(Subcommand)]
enum ApikeyAction {
    /// Create a new API key for an owner
    Create {
        /// Owner ID
        #[arg(long)]
        owner: String,
        /// Key name/description
        #[arg(long)]
        name: Option<String>,
    },
    /// List API keys
    List {
        /// Filter by owner ID
        #[arg(long)]
        owner: Option<String>,
    },
    /// Revoke an API key
    Revoke {
        /// API key ID
        id: String,
    },
}

#[derive(Subcommand)]
enum VaultAction {
    /// Store a secret (encrypts with AES-256-GCM)
    Store {
        /// Secret name
        #[arg(long)]
        name: String,
        /// Secret value
        #[arg(long)]
        value: String,
        /// Owner ID
        #[arg(long, default_value = "default")]
        owner: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Retrieve and decrypt a secret
    Get {
        /// Secret name
        #[arg(long)]
        name: String,
        /// Owner ID
        #[arg(long, default_value = "default")]
        owner: String,
    },
    /// List all secret names (no values)
    List {
        /// Owner ID
        #[arg(long, default_value = "default")]
        owner: String,
    },
    /// Delete a secret
    Delete {
        /// Secret name
        #[arg(long)]
        name: String,
        /// Owner ID
        #[arg(long, default_value = "default")]
        owner: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    // Tracing subscriber with log capture layer (added lazily when serve starts)
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "alaz=debug".parse().expect("static filter string is valid"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().json())
        .with(alaz_server::log_capture::LogCaptureLayer)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => cmd_serve().await,
        Commands::Migrate { status, dry_run } => cmd_migrate(status, dry_run).await,
        Commands::Hook { action } => match action {
            HookAction::Start {
                project,
                url,
                api_key,
            } => cmd_hook_start(project, url, api_key).await,
            HookAction::Stop {
                session_id,
                transcript,
                project,
                url,
                api_key,
            } => cmd_hook_stop(session_id, transcript, project, url, api_key).await,
            HookAction::Compact {
                session_id,
                project,
            } => cmd_hook_compact(session_id, project).await,
            HookAction::Context { url } => cmd_hook_context(url).await,
        },
        Commands::RaptorRebuild { project } => cmd_raptor_rebuild(project).await,
        Commands::Owner { action } => cmd_owner(action).await,
        Commands::Device { action } => cmd_device(action).await,
        Commands::Apikey { action } => cmd_apikey(action).await,
        Commands::Vault { action } => cmd_vault(action).await,
        Commands::Audit {
            owner,
            event,
            limit,
        } => cmd_audit(owner, event, limit).await,
        Commands::GenerateClaudeMd {
            project,
            output,
            max_chars,
        } => cmd_generate_claude_md(project, output, max_chars).await,
    }
}

// --- Subcommand implementations ---

async fn cmd_serve() -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let listen_addr = config.listen_addr.clone();
    let state = alaz_server::AppState::new(config).await?;

    // Auto-run migrations on startup
    alaz_db::run_migrations(&state.pool).await?;

    // Initialize log capture (background writer for structured_logs table)
    let _ = alaz_server::log_capture::init_log_capture(state.pool.clone());
    info!("log capture initialized — warn/error events now persisted");

    // Spawn background jobs with supervision — log panics instead of silent death
    let mut job_handles: Vec<(&str, tokio::task::JoinHandle<()>)> = Vec::new();

    job_handles.push((
        "embedding_backfill",
        tokio::spawn(alaz_server::jobs::embedding_backfill_job(
            state.pool.clone(),
            state.qdrant.clone(),
            state.embedding.clone(),
            state.colbert.clone(),
            state.metrics.clone(),
        )),
    ));
    info!("spawned embedding backfill job (every 5 min)");

    job_handles.push((
        "graph_decay",
        tokio::spawn(alaz_server::jobs::graph_decay_job(state.pool.clone())),
    ));
    info!("spawned graph decay job (every 6 hours)");

    job_handles.push((
        "memory_decay",
        tokio::spawn(alaz_server::jobs::memory_decay_job(
            state.pool.clone(),
            state.qdrant.clone(),
            state.metrics.clone(),
        )),
    ));
    info!("spawned memory decay job (every 6 hours)");

    job_handles.push((
        "feedback_aggregation",
        tokio::spawn(alaz_server::jobs::feedback_aggregation_job(
            state.pool.clone(),
        )),
    ));
    info!("spawned feedback aggregation job (every 12 hours)");

    job_handles.push((
        "weight_learning",
        tokio::spawn(alaz_server::jobs::weight_learning_job(state.pool.clone())),
    ));
    info!("spawned weight learning job (every 7 days)");

    job_handles.push((
        "consolidation",
        tokio::spawn(alaz_server::jobs::consolidation_job(
            state.pool.clone(),
            state.llm.clone(),
            state.embedding.clone(),
            state.qdrant.clone(),
            state.metrics.clone(),
        )),
    ));
    info!("spawned consolidation job (every 7 days)");

    job_handles.push((
        "learning_queue",
        tokio::spawn(alaz_server::jobs::learning_queue_job(
            state.pool.clone(),
            state.llm.clone(),
            state.embedding.clone(),
            state.qdrant.clone(),
            state.metrics.clone(),
        )),
    ));
    info!("spawned learning queue job (debounced, 3-min cooldown)");

    job_handles.push((
        "error_aggregation",
        tokio::spawn(alaz_server::jobs::error_aggregation_job(state.pool.clone())),
    ));
    info!("spawned error aggregation job (every 60s)");

    job_handles.push((
        "alert_evaluation",
        tokio::spawn(alaz_server::jobs::alert_evaluation_job(state.pool.clone())),
    ));
    info!("spawned alert evaluation job (every 60s)");

    // Supervisor: monitor all job handles and log if any exit or panic
    tokio::spawn(async move {
        for (name, handle) in job_handles {
            let job_name = name.to_string();
            tokio::spawn(async move {
                match handle.await {
                    Ok(()) => {
                        tracing::error!(job = %job_name, "background job exited unexpectedly")
                    }
                    Err(e) => {
                        tracing::error!(job = %job_name, error = %e, "background job panicked")
                    }
                }
            });
        }
    });

    let router = alaz_server::build_router(state);

    info!(addr = %listen_addr, "starting Alaz server");
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;

    // Graceful shutdown: wait for SIGTERM/SIGINT, then allow in-flight requests to complete
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown signal received, finishing in-flight requests...");
        })
        .await?;

    info!("server shut down gracefully");
    Ok(())
}

async fn cmd_migrate(status: bool, dry_run: bool) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    if status {
        let infos = alaz_db::migration_status(&pool).await?;
        println!("Migration Status:");
        println!("{:<10} {:<35} Status", "Version", "Name");
        println!("{}", "-".repeat(60));
        for m in &infos {
            let badge = if m.applied {
                "✓ applied"
            } else {
                "○ pending"
            };
            println!("{:<10} {:<35} {}", m.version, m.name, badge);
        }
        let applied = infos.iter().filter(|m| m.applied).count();
        let pending = infos.len() - applied;
        println!();
        println!("{applied} applied, {pending} pending");
    } else if dry_run {
        let pending = alaz_db::migrations_pending(&pool).await?;
        if pending.is_empty() {
            println!("All migrations are up to date — nothing to apply.");
        } else {
            println!("Pending migrations (would be applied):");
            for m in &pending {
                println!("  {} {}", m.version, m.name);
            }
            println!("\n{} migration(s) would be applied.", pending.len());
        }
    } else {
        let applied = alaz_db::run_migrations(&pool).await?;
        println!("migrations completed successfully ({applied} newly applied)");
    }

    Ok(())
}

async fn cmd_hook_start(
    project: Option<String>,
    url: Option<String>,
    api_key: Option<String>,
) -> Result<()> {
    // Read hook input from stdin (Claude Code pipes JSON)
    let hook_input = read_hook_input();

    // Check .alaz marker — skip context injection if not present
    let cwd = hook_input.cwd.as_deref().unwrap_or(".");
    if !has_alaz_marker(cwd) {
        info!(cwd, "hook start: no .alaz marker found, skipping");
        return Ok(());
    }

    // Resolve project: CLI arg > stdin cwd > default "."
    let project_name =
        project.or_else(|| hook_input.cwd.as_ref().map(|c| project_name_from_cwd(c)));
    let project_path = project_name.as_deref().unwrap_or(".");

    // Auto-regenerate CLAUDE.md if stale (>24h) or missing
    let claude_md_path = std::path::Path::new(cwd).join("CLAUDE.md");
    let needs_regen = if claude_md_path.exists() {
        std::fs::metadata(&claude_md_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs() > 86400)
            .unwrap_or(true)
    } else {
        true
    };

    if needs_regen {
        let project_for_regen = project_path.to_string();
        let output_path = claude_md_path.to_string_lossy().to_string();
        let db_url = std::env::var("DATABASE_URL").unwrap_or_default();
        if !db_url.is_empty() {
            tokio::spawn(async move {
                match alaz_db::create_pool(&db_url).await {
                    Ok(pool) => {
                        match generate::generate_claude_md(&pool, &project_for_regen, 16000).await {
                            Ok(md) => {
                                let _ = std::fs::write(&output_path, &md);
                                tracing::info!("Regenerated CLAUDE.md for {}", project_for_regen);
                            }
                            Err(e) => tracing::debug!("CLAUDE.md regen failed: {}", e),
                        }
                    }
                    Err(e) => tracing::debug!("CLAUDE.md regen pool failed: {}", e),
                }
            });
        }
    }

    if let Some(base_url) = url {
        // Remote mode: call Alaz HTTP API
        let api_key = api_key.unwrap_or_default();
        match remote_hook_start(&base_url, &api_key, project_path).await {
            Ok(context) => println!("{context}"),
            Err(e) => {
                info!(error = %e, "hook start (remote): context injection failed");
                println!();
            }
        }
    } else {
        // Local mode: direct DB connection
        let config = alaz_core::AppConfig::from_env()?;
        let pool = alaz_db::create_pool(&config.database_url).await?;
        let injector = alaz_intel::ContextInjector::new(pool);
        match injector.build_context(project_path).await {
            Ok(context) => {
                println!("{context}");
            }
            Err(e) => {
                info!(error = %e, "hook start: context injection failed, outputting empty context");
                println!();
            }
        }
    }

    Ok(())
}

async fn cmd_hook_stop(
    session_id: Option<String>,
    transcript: Option<String>,
    project: Option<String>,
    url: Option<String>,
    api_key: Option<String>,
) -> Result<()> {
    // Read hook input from stdin (Claude Code pipes JSON)
    let hook_input = read_hook_input();

    // Check .alaz marker — skip learning pipeline if not present
    let cwd = hook_input.cwd.as_deref().unwrap_or(".");
    if !has_alaz_marker(cwd) {
        info!(
            cwd,
            "hook stop: no .alaz marker found, skipping learning pipeline"
        );
        return Ok(());
    }

    // Resolve session_id: CLI arg > stdin JSON (clone to avoid partial move)
    let sid = session_id.or_else(|| hook_input.session_id.clone());

    // Resolve transcript: CLI arg > read file from stdin transcript_path
    let tx = if let Some(t) = transcript {
        Some(t)
    } else if let Some(ref path) = hook_input.transcript_path {
        match read_transcript_file(path) {
            Ok(t) if !t.is_empty() => Some(t),
            Ok(_) => {
                info!("hook stop: transcript file was empty");
                None
            }
            Err(e) => {
                info!(error = %e, path = %path, "hook stop: failed to read transcript file");
                None
            }
        }
    } else {
        None
    };

    // Resolve project: CLI arg > stdin cwd
    let project_name =
        project.or_else(|| hook_input.cwd.as_ref().map(|c| project_name_from_cwd(c)));

    if let (Some(sid), Some(tx)) = (sid, tx) {
        info!(session_id = %sid, transcript_len = tx.len(), project = ?project_name, "hook stop: starting learning pipeline");

        if let Some(base_url) = url {
            let api_key = api_key.unwrap_or_default();
            hook_stop_remote(&base_url, &api_key, &sid, &tx, project_name.as_deref()).await?;
        } else {
            hook_stop_local(&sid, &tx, project_name.as_deref(), &hook_input).await?;
        }
    } else {
        info!("hook stop: no session_id or transcript available, skipping learning");
    }

    Ok(())
}

async fn hook_stop_remote(
    base_url: &str,
    api_key: &str,
    sid: &str,
    tx: &str,
    project_name: Option<&str>,
) -> Result<()> {
    // Best-effort git commit ingestion before learning
    // (uses current working directory of the hook process)
    if let Ok(cwd) = std::env::current_dir()
        && let Some(repo_root) = alaz_intel::git_ingest::find_repo_root(&cwd)
    {
        // Look back 6 hours — covers most session durations
        let since_epoch = chrono::Utc::now().timestamp() - 6 * 3600;
        match alaz_intel::git_ingest::read_commits_since(&repo_root, since_epoch, 50) {
            Ok(commits) if !commits.is_empty() => {
                if let Err(e) =
                    push_commits_remote(base_url, api_key, project_name, Some(sid), commits).await
                {
                    info!(error = %e, "remote git ingest failed (non-fatal)");
                }
            }
            _ => {}
        }
    }

    match remote_hook_stop(base_url, api_key, sid, tx, project_name).await {
        Ok(result) => println!("{result}"),
        Err(e) => {
            info!(error = %e, "hook stop (remote): learning failed");
            eprintln!("learning failed: {e}");
        }
    }
    Ok(())
}

async fn push_commits_remote(
    base_url: &str,
    api_key: &str,
    project: Option<&str>,
    session_id: Option<&str>,
    commits: Vec<alaz_intel::git_ingest::GitCommit>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let url = format!(
        "{}/api/v1/git/ingest-commits",
        base_url.trim_end_matches('/')
    );

    let mut body = serde_json::json!({ "commits": commits });
    if let Some(p) = project {
        body["project"] = serde_json::Value::String(p.to_string());
    }
    if let Some(s) = session_id {
        body["session_id"] = serde_json::Value::String(s.to_string());
    }

    let resp = client
        .post(&url)
        .header("X-API-Key", api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("git ingest failed: HTTP {}", resp.status());
    }
    Ok(())
}

async fn hook_stop_local(
    sid: &str,
    tx: &str,
    project_name: Option<&str>,
    hook_input: &HookInput,
) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    let project_id = if let Some(name) = project_name {
        alaz_db::repos::ProjectRepo::get_or_create(&pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    // Auto-checkpoint: save session state before learning
    if let Ok(_session) =
        alaz_db::repos::SessionRepo::ensure_exists(&pool, sid, project_id.as_deref()).await
    {
        let checkpoint_data = serde_json::json!({
            "source": "auto_stop",
            "project": project_name,
            "cwd": hook_input.cwd,
            "transcript_len": tx.len(),
            "last_assistant_message": hook_input.last_assistant_message,
        });
        match alaz_db::repos::SessionRepo::save_checkpoint(&pool, sid, &checkpoint_data).await {
            Ok(cp) => info!(checkpoint_id = %cp.id, "hook stop: auto-checkpoint saved"),
            Err(e) => info!(error = %e, "hook stop: auto-checkpoint failed (non-fatal)"),
        }
    }

    // Ensure session exists before saving transcript
    let _ = alaz_db::repos::SessionRepo::ensure_exists(&pool, sid, project_id.as_deref()).await;

    // Save transcript text for FTS session search
    let message_count = tx.matches("[USER]:").count() + tx.matches("[ASSISTANT]:").count();
    if let Err(e) = alaz_db::repos::SessionRepo::update_transcript_text(
        &pool,
        sid,
        tx,
        None, // summary will be generated by reflection
        message_count as i32,
    )
    .await
    {
        info!(error = %e, "hook stop: transcript save failed (non-fatal)");
    }

    // Git commit ingestion: capture what actually happened during the session.
    if let Some(cwd) = hook_input.cwd.as_ref() {
        let cwd_path = std::path::Path::new(cwd);
        if let Some(repo_root) = alaz_intel::git_ingest::find_repo_root(cwd_path) {
            // Use session creation time as the "since" timestamp, fallback to 4h ago
            let since_epoch = alaz_db::repos::SessionRepo::get_session_start_epoch(&pool, sid)
                .await
                .unwrap_or_else(|_| chrono::Utc::now().timestamp() - 4 * 3600);

            match alaz_intel::git_ingest::ingest_commits_for_session(
                &pool,
                &repo_root,
                since_epoch,
                project_id.as_deref(),
                Some(sid),
                50,
            )
            .await
            {
                Ok(s) => {
                    if s.commits_read > 0 {
                        info!(
                            read = s.commits_read,
                            created = s.episodes_created,
                            dupes = s.duplicates_skipped,
                            "git ingest: commits processed"
                        );
                    }
                }
                Err(e) => info!(error = %e, "git ingest failed (non-fatal)"),
            }
        }
    }

    // Enqueue for debounced learning (same as remote mode).
    // The server's learning_queue_job will pick it up after the cooldown.
    match alaz_db::repos::LearningQueueRepo::enqueue(
        &pool,
        sid,
        project_id.as_deref(),
        tx,
        message_count as i32,
    )
    .await
    {
        Ok(queue_id) => {
            let result = serde_json::json!({
                "session_id": sid,
                "queue_id": queue_id,
                "status": "queued",
                "message": "Learning queued. Server will process after 3-min cooldown.",
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Err(e) => {
            info!(error = %e, "hook stop: queue failed");
            eprintln!("learning queue failed: {e}");
        }
    }

    Ok(())
}

async fn cmd_hook_compact(session_id: String, project: Option<String>) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    let project_id = if let Some(ref name) = project {
        alaz_db::repos::ProjectRepo::get_or_create(&pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let restorer = alaz_intel::CompactRestorer::new(pool);
    match restorer
        .build_restore_context(&session_id, project_id.as_deref(), None)
        .await
    {
        Ok(result) => {
            println!("{}", result.formatted_output);
        }
        Err(e) => {
            info!(error = %e, "hook compact: restore context failed");
            eprintln!("compact restore failed: {e}");
        }
    }

    Ok(())
}

#[derive(PartialEq)]
enum TriggerMode {
    Error(String),    // Error text to search
    FileOpen(String), // File path being opened
    Skip,
}

async fn cmd_hook_context(url: String) -> Result<()> {
    use std::fmt::Write;

    let hook_input = read_hook_input_raw();
    if hook_input.is_empty() {
        return Ok(());
    }

    let parsed: serde_json::Value = serde_json::from_str(&hook_input).unwrap_or_default();
    let cwd = parsed.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");
    if !has_alaz_marker(cwd) {
        return Ok(());
    }

    let tool_name = parsed
        .get("tool_name")
        .or_else(|| parsed.get("tool"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tool_input = parsed
        .get("tool_input")
        .or_else(|| parsed.get("input"))
        .unwrap_or(&serde_json::Value::Null);
    let tool_output = parsed
        .get("tool_output")
        .or_else(|| parsed.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Determine trigger mode from tool type
    let mode = match tool_name {
        "Bash" => {
            let lower = tool_output.to_lowercase();
            let error_keywords = [
                "error",
                "panic",
                "failed",
                "exception",
                "fatal",
                "cannot find",
                "not found",
                "no such file",
                "denied",
            ];
            if error_keywords.iter().any(|kw| lower.contains(kw)) {
                // Extract meaningful keywords, join with OR for FTS
                let stop_words = [
                    "the", "and", "for", "not", "was", "error", "with", "from", "that", "this",
                    "can", "cannot", "could", "did", "does", "find", "found", "module", "file",
                    "such", "failed", "fatal", "missing",
                ];
                let keywords: Vec<String> = tool_output
                    .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                    .filter(|w| w.len() >= 3)
                    .filter(|w| !stop_words.contains(&w.to_lowercase().as_str()))
                    .take(4)
                    .map(|w| w.to_string())
                    .collect();
                let query = keywords.join(" OR ");
                if query.is_empty() {
                    TriggerMode::Skip
                } else {
                    TriggerMode::Error(query)
                }
            } else {
                TriggerMode::Skip
            }
        }
        "Read" => {
            let file_path = tool_input
                .get("file_path")
                .or_else(|| tool_input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !file_path.is_empty() {
                TriggerMode::FileOpen(file_path.to_string())
            } else {
                TriggerMode::Skip
            }
        }
        _ => TriggerMode::Skip,
    };

    if mode == TriggerMode::Skip {
        return Ok(());
    }

    // Cooldown check via state file
    let state_path = "/tmp/alaz-hook-state.json";
    let mut state: serde_json::Value = std::fs::read_to_string(state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Project-level cooldown: 60 seconds
    if let Some(last) = state.get("last_injection").and_then(|v| v.as_u64())
        && now.saturating_sub(last) < 60
    {
        return Ok(());
    }

    // File-level: skip if already seen in this session
    if let TriggerMode::FileOpen(ref path) = mode
        && let Some(files) = state.get("seen_files").and_then(|v| v.as_array())
        && files.iter().any(|f| f.as_str() == Some(path.as_str()))
    {
        return Ok(());
    }

    // Search Alaz
    let base_url = url.trim_end_matches('/');
    let api_key = std::env::var("ALAZ_API_KEY")
        .unwrap_or_else(|_| "alaz_vs8568tzyy3hlmf5hswv1t11".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;

    let context_text = match &mode {
        TriggerMode::Error(error_text) => {
            // Fast FTS search for error (hybrid is too slow for 3s hook timeout)
            let endpoint = format!("{}/api/v1/search", base_url);
            let resp = client
                .get(&endpoint)
                .header("X-API-Key", &api_key)
                .query(&[("query", error_text.as_str()), ("limit", "3")])
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    let results: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                    let mut out = String::new();
                    for item in results.iter().take(3) {
                        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let _ = writeln!(out, "- **{}**", title);
                    }
                    out
                }
                _ => String::new(),
            }
        }
        TriggerMode::FileOpen(file_path) => {
            // Search episodes by file + get project constraints
            let endpoint = format!("{}/api/v1/episodes/by-files", base_url);
            let resp = client
                .post(&endpoint)
                .header("X-API-Key", &api_key)
                .json(&serde_json::json!({
                    "files": [file_path],
                    "limit": 3
                }))
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    let results: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                    let mut out = String::new();
                    for item in results.iter().take(3) {
                        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let severity = item
                            .get("severity")
                            .and_then(|v| v.as_str())
                            .unwrap_or("info");
                        let _ = writeln!(out, "- [{}] {}", severity, title);
                    }
                    out
                }
                _ => String::new(),
            }
        }
        TriggerMode::Skip => String::new(),
    };

    // Only output if we found something
    if !context_text.is_empty() {
        let mut output = String::from("<alaz-context source=\"live\">\n");
        // Truncate total output to 500 chars
        if context_text.len() > 500 {
            let truncated = &context_text[..context_text.floor_char_boundary(500)];
            output.push_str(truncated);
        } else {
            output.push_str(&context_text);
        }
        output.push_str("</alaz-context>");
        println!("{}", output);

        // Update cooldown state
        state["last_injection"] = serde_json::json!(now);

        // Track seen files (max 100)
        if let TriggerMode::FileOpen(ref path) = mode {
            let mut files = state
                .get("seen_files")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            files.push(serde_json::json!(path));
            if files.len() > 100 {
                files.remove(0);
            }
            state["seen_files"] = serde_json::json!(files);
        }

        let _ = std::fs::write(
            state_path,
            serde_json::to_string(&state).unwrap_or_default(),
        );
    }

    Ok(())
}

async fn cmd_raptor_rebuild(project: Option<String>) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;
    let llm = std::sync::Arc::new(alaz_intel::LlmClient::with_base_url(
        &config.llm_api_key,
        &config.llm_model,
        &config.llm_base_url,
    ));
    let embedding = std::sync::Arc::new(alaz_intel::EmbeddingService::new(
        &config.ollama_url,
        &config.text_embed_model,
    ));
    let qdrant = std::sync::Arc::new(
        alaz_vector::QdrantManager::with_text_dim(&config.qdrant_url, config.text_embed_dim)
            .await?,
    );

    let project_id = if let Some(ref name) = project {
        alaz_db::repos::ProjectRepo::get_or_create(&pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let builder = alaz_intel::RaptorBuilder::new(pool, llm, embedding, qdrant);
    let tree = builder.rebuild_tree(project_id.as_deref()).await?;
    println!("{}", serde_json::to_string_pretty(&tree)?);

    Ok(())
}

async fn cmd_owner(action: OwnerAction) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    match action {
        OwnerAction::Create { username, password } => {
            let password_hash = alaz_auth::hash_password(&password)?;
            let owner = alaz_db::repos::OwnerRepo::create(&pool, &username, &password_hash).await?;
            println!("owner created:");
            println!("  id:       {}", owner.id);
            println!("  username: {}", owner.username);
        }
        OwnerAction::List => {
            let owners = alaz_db::repos::OwnerRepo::list(&pool).await?;
            if owners.is_empty() {
                println!("no owners found");
            } else {
                for o in owners {
                    println!("  {} | {} | created {}", o.id, o.username, o.created_at);
                }
            }
        }
    }

    Ok(())
}

async fn cmd_device(action: DeviceAction) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    match action {
        DeviceAction::List { owner } => {
            let devices = alaz_db::repos::DeviceRepo::list(&pool, owner.as_deref()).await?;
            if devices.is_empty() {
                println!("no devices found");
            } else {
                for d in devices {
                    let status = if d.trusted { "TRUSTED" } else { "UNTRUSTED" };
                    let name = d.name.unwrap_or_else(|| "-".to_string());
                    let last_seen = d
                        .last_seen_at
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "never".to_string());
                    println!(
                        "  {} | {} | {} | {} | last seen: {}",
                        d.id, d.fingerprint, name, status, last_seen
                    );
                }
            }
        }
        DeviceAction::Approve { id } => {
            let device = alaz_db::repos::DeviceRepo::approve(&pool, &id).await?;
            println!("device approved:");
            println!("  id:          {}", device.id);
            println!("  fingerprint: {}", device.fingerprint);
            println!("  trusted:     {}", device.trusted);
        }
        DeviceAction::Revoke { id } => {
            let device = alaz_db::repos::DeviceRepo::revoke(&pool, &id).await?;
            println!("device trust revoked:");
            println!("  id:          {}", device.id);
            println!("  fingerprint: {}", device.fingerprint);
            println!("  trusted:     {}", device.trusted);
        }
        DeviceAction::Delete { id } => {
            alaz_db::repos::DeviceRepo::delete(&pool, &id).await?;
            println!("device deleted: {id}");
        }
    }

    Ok(())
}

async fn cmd_apikey(action: ApikeyAction) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    match action {
        ApikeyAction::Create { owner, name } => {
            // Generate a random API key
            let raw_key = format!("alaz_{}", cuid2::create_id());
            let key_hash = alaz_auth::hash_key(&raw_key);
            let key = alaz_db::repos::ApiKeyRepo::create(&pool, &owner, &key_hash, name.as_deref())
                .await?;
            println!("API key created:");
            println!("  id:   {}", key.id);
            println!("  key:  {raw_key}");
            println!("  name: {}", key.name.unwrap_or_else(|| "-".to_string()));
            println!();
            println!("  IMPORTANT: Save this key now — it cannot be retrieved later.");
        }
        ApikeyAction::List { owner } => {
            let keys = alaz_db::repos::ApiKeyRepo::list(&pool, owner.as_deref()).await?;
            if keys.is_empty() {
                println!("no API keys found");
            } else {
                for k in keys {
                    let name = k.name.unwrap_or_else(|| "-".to_string());
                    let last_used = k
                        .last_used_at
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "never".to_string());
                    println!(
                        "  {} | {} | owner: {} | last used: {}",
                        k.id, name, k.owner_id, last_used
                    );
                }
            }
        }
        ApikeyAction::Revoke { id } => {
            alaz_db::repos::ApiKeyRepo::revoke(&pool, &id).await?;
            println!("API key revoked: {id}");
        }
    }

    Ok(())
}

async fn cmd_vault(action: VaultAction) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;
    let vault_key = config
        .vault_master_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow::anyhow!("VAULT_MASTER_KEY not set"))?;
    let crypto = alaz_auth::VaultCrypto::from_hex_key(vault_key)?;

    match action {
        VaultAction::Store {
            name,
            value,
            owner,
            description,
        } => {
            let (encrypted, nonce) = crypto.encrypt(value.as_bytes())?;
            let secret = alaz_db::repos::VaultRepo::store(
                &pool,
                &owner,
                &name,
                &encrypted,
                &nonce,
                description.as_deref(),
            )
            .await?;
            println!("secret stored:");
            println!("  id:   {}", secret.id);
            println!("  name: {}", secret.name);
        }
        VaultAction::Get { name, owner } => {
            let secret = alaz_db::repos::VaultRepo::get_by_name(&pool, &owner, &name).await?;
            let plaintext = crypto.decrypt(&secret.encrypted_value, &secret.nonce)?;
            let value = String::from_utf8(plaintext)?;
            println!("{value}");
        }
        VaultAction::List { owner } => {
            let secrets = alaz_db::repos::VaultRepo::list(&pool, &owner).await?;
            if secrets.is_empty() {
                println!("no secrets found");
            } else {
                for s in secrets {
                    let desc = s.description.unwrap_or_else(|| "-".to_string());
                    println!("  {} | {} | updated {}", s.name, desc, s.updated_at);
                }
            }
        }
        VaultAction::Delete { name, owner } => {
            alaz_db::repos::VaultRepo::delete(&pool, &owner, &name).await?;
            println!("secret deleted: {name}");
        }
    }

    Ok(())
}

async fn cmd_audit(owner: Option<String>, event: Option<String>, limit: i64) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;
    let logs =
        alaz_db::repos::AuditRepo::list(&pool, owner.as_deref(), event.as_deref(), limit).await?;
    if logs.is_empty() {
        println!("no audit logs found");
    } else {
        for log in logs {
            let owner_id = log.owner_id.unwrap_or_else(|| "-".to_string());
            println!(
                "  {} | {} | {} | {}",
                log.created_at, owner_id, log.event, log.details
            );
        }
    }

    Ok(())
}

async fn cmd_generate_claude_md(
    project: Option<String>,
    output: Option<String>,
    max_chars: usize,
) -> Result<()> {
    let config = alaz_core::AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;

    // Detect project name: --project arg > cwd-based detection
    let project_name = if let Some(p) = project {
        p
    } else {
        let cwd = std::env::current_dir()?;
        let cwd_str = cwd.to_string_lossy().to_string();
        if !has_alaz_marker(&cwd_str) {
            anyhow::bail!(
                "No .alaz marker found. Specify --project or add .alaz to your project root."
            );
        }
        project_name_from_cwd(&cwd_str)
    };

    info!(project = %project_name, max_chars, "generating CLAUDE.md");

    let generated = generate::generate_claude_md(&pool, &project_name, max_chars).await?;

    // Determine output path
    let output_path = if let Some(p) = output {
        std::path::PathBuf::from(p)
    } else {
        std::env::current_dir()?.join("CLAUDE.md")
    };

    // If the file already exists, try to preserve user content above the marker
    let marker = format!("# {} \u{2014} Alaz Context", project_name);
    let final_content = if output_path.exists() {
        let existing = std::fs::read_to_string(&output_path)?;
        if let Some(marker_pos) = existing.find(&marker) {
            // Preserve everything before the marker, replace from marker onward
            let prefix = &existing[..marker_pos];
            format!("{}{}", prefix, generated)
        } else {
            // Marker not found: append after existing content
            format!("{}\n\n{}", existing.trim_end(), generated)
        }
    } else {
        generated
    };

    std::fs::write(&output_path, &final_content)?;

    let sections_count = final_content.matches("\n## ").count();
    println!(
        "wrote {} chars ({} sections) to {}",
        final_content.len(),
        sections_count,
        output_path.display()
    );

    Ok(())
}

// --- Claude Code hook stdin JSON parsing ---

/// Claude Code pipes JSON to hook commands via stdin.
/// This struct captures the fields we need.
#[derive(Deserialize, Default, Debug)]
struct HookInput {
    session_id: Option<String>,
    transcript_path: Option<String>,
    cwd: Option<String>,
    // Stop event extras
    last_assistant_message: Option<String>,
}

/// Read raw stdin content as a string (for custom parsing).
fn read_hook_input_raw() -> String {
    use std::io::Read;
    let mut input = String::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let _ = handle.read_to_string(&mut input);
    input
}

/// Try to read and parse hook JSON from stdin (non-blocking).
/// Returns default if stdin is empty or not valid JSON.
fn read_hook_input() -> HookInput {
    use std::io::Read;

    // Check if stdin has data (non-blocking)
    let mut input = String::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();

    // Try to read with a small buffer first
    match handle.read_to_string(&mut input) {
        Ok(0) => HookInput::default(),
        Ok(_) => serde_json::from_str(&input).unwrap_or_else(|_| {
            info!("hook stdin was not valid JSON, ignoring");
            HookInput::default()
        }),
        Err(_) => HookInput::default(),
    }
}

/// Read a JSONL transcript file and convert to text for the learning pipeline.
/// Claude Code transcript format: each line is JSON with `type` (user/assistant/system),
/// and `message` object containing `role` and `content` (array of content blocks).
pub(crate) fn read_transcript_file(path: &str) -> Result<String> {
    // pub(crate) for testing
    let content = std::fs::read_to_string(path)?;

    // If the file is plain text (not JSONL), return as-is
    if !content.starts_with('{') {
        return Ok(content);
    }

    let mut transcript = String::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Only process user/assistant messages (skip progress, file-history-snapshot, etc.)
        let msg_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if msg_type != "user" && msg_type != "assistant" && msg_type != "system" {
            continue;
        }

        // Claude Code format: content is nested in entry["message"]["content"]
        let message = entry.get("message");
        let role = message
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("role").and_then(|v| v.as_str()))
            .unwrap_or(msg_type);

        // Try nested message.content first, then top-level content
        let content_val = message
            .and_then(|m| m.get("content"))
            .or_else(|| entry.get("content"));

        let text_str = match content_val {
            Some(v) if v.is_string() => v.as_str().unwrap_or("").to_string(),
            Some(v) if v.is_array() => {
                // Content blocks: [{"type":"text","text":"..."}, {"type":"tool_use",...}]
                v.as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            }
            _ => continue,
        };

        if !text_str.is_empty() {
            let tag = match role {
                "human" | "user" => "[USER]",
                "assistant" => "[ASSISTANT]",
                "system" => "[SYSTEM]",
                _ => continue,
            };
            transcript.push_str(&format!("{}: {}\n\n", tag, text_str));
        }
    }

    if transcript.is_empty() {
        Ok(content)
    } else {
        Ok(transcript)
    }
}

/// Check if the given directory (or any parent) contains a `.alaz` marker file.
/// Only projects with this marker will have hooks (context injection + learning) active.
pub(crate) fn has_alaz_marker(cwd: &str) -> bool {
    // pub(crate) for testing
    let mut path = std::path::Path::new(cwd);
    loop {
        if path.join(".alaz").exists() {
            return true;
        }
        match path.parent() {
            Some(parent) if parent != path => path = parent,
            _ => return false,
        }
    }
}

/// Derive project name from cwd path (last component).
pub(crate) fn project_name_from_cwd(cwd: &str) -> String {
    // pub(crate) for testing
    std::path::Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

// --- Remote hook helpers (HTTP API calls instead of direct DB) ---

async fn remote_hook_start(base_url: &str, api_key: &str, project_path: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let url = format!("{}/api/v1/context", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("X-API-Key", api_key)
        .query(&[("path", project_path)])
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("remote hook start failed: HTTP {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    Ok(body["context"].as_str().unwrap_or("").to_string())
}

async fn remote_hook_stop(
    base_url: &str,
    api_key: &str,
    session_id: &str,
    transcript: &str,
    project: Option<&str>,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let url = format!(
        "{}/api/v1/sessions/{}/learn",
        base_url.trim_end_matches('/'),
        session_id
    );

    let mut body = serde_json::json!({
        "transcript": transcript,
    });
    if let Some(p) = project {
        body["project"] = serde_json::Value::String(p.to_string());
    }

    let resp = client
        .post(&url)
        .header("X-API-Key", api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("remote hook stop failed: HTTP {}", resp.status());
    }

    let result: serde_json::Value = resp.json().await?;
    Ok(serde_json::to_string_pretty(&result)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("alaz_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- has_alaz_marker ---

    #[test]
    fn marker_present_returns_true() {
        let dir = temp_dir("marker_yes");
        fs::write(dir.join(".alaz"), "").unwrap();
        assert!(has_alaz_marker(dir.to_str().unwrap()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn marker_absent_returns_false() {
        let dir = temp_dir("marker_no");
        assert!(!has_alaz_marker(dir.to_str().unwrap()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn marker_in_parent_returns_true() {
        let parent = temp_dir("marker_parent");
        fs::write(parent.join(".alaz"), "").unwrap();
        let child = parent.join("subdir").join("deep");
        fs::create_dir_all(&child).unwrap();
        assert!(has_alaz_marker(child.to_str().unwrap()));
        fs::remove_dir_all(&parent).ok();
    }

    // --- project_name_from_cwd ---

    #[test]
    fn project_name_normal_path() {
        assert_eq!(project_name_from_cwd("/home/user/Projects/Alaz"), "Alaz");
    }

    #[test]
    fn project_name_root_path() {
        let name = project_name_from_cwd("/");
        assert_eq!(name, "unknown");
    }

    #[test]
    fn project_name_trailing_slash() {
        let name = project_name_from_cwd("/home/user/myproject/");
        assert!(!name.is_empty());
    }

    #[test]
    fn project_name_relative_path() {
        assert_eq!(project_name_from_cwd("./myproject"), "myproject");
    }

    // --- read_transcript_file ---

    #[test]
    fn read_plain_text_file() {
        let dir = temp_dir("tx_plain");
        let path = dir.join("session.txt");
        fs::write(&path, "Hello, this is a plain transcript.").unwrap();
        let result = read_transcript_file(path.to_str().unwrap()).unwrap();
        assert_eq!(result, "Hello, this is a plain transcript.");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_jsonl_user_assistant() {
        let dir = temp_dir("tx_jsonl");
        let path = dir.join("session.jsonl");
        let content = r#"{"type":"user","message":{"role":"human","content":[{"type":"text","text":"Hello"}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hi there!"}]}}"#;
        fs::write(&path, content).unwrap();

        let result = read_transcript_file(path.to_str().unwrap()).unwrap();
        assert!(result.contains("[USER]: Hello"));
        assert!(result.contains("[ASSISTANT]: Hi there!"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_empty_file() {
        let dir = temp_dir("tx_empty");
        let path = dir.join("empty.txt");
        fs::write(&path, "").unwrap();
        let result = read_transcript_file(path.to_str().unwrap()).unwrap();
        assert!(result.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_jsonl_non_message_types_returns_raw() {
        let dir = temp_dir("tx_nonmsg");
        let path = dir.join("session.jsonl");
        let content = r#"{"type":"progress","data":"compiling..."}
{"type":"file-history-snapshot","files":[]}
{"type":"tool_result","output":"ok"}"#;
        fs::write(&path, content).unwrap();

        let result = read_transcript_file(path.to_str().unwrap()).unwrap();
        // No user/assistant messages → transcript is empty → returns raw content
        assert!(result.contains("progress"));
        assert!(result.contains("file-history-snapshot"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_jsonl_string_content() {
        let dir = temp_dir("tx_str_content");
        let path = dir.join("session.jsonl");
        let content = r#"{"type":"user","message":{"role":"human","content":"simple string"}}"#;
        fs::write(&path, content).unwrap();

        let result = read_transcript_file(path.to_str().unwrap()).unwrap();
        assert!(result.contains("[USER]: simple string"));
        fs::remove_dir_all(&dir).ok();
    }
}
