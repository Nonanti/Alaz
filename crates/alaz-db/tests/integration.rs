//! Integration tests for alaz-db.
//!
//! These tests require a running PostgreSQL instance. By default they connect to:
//!   `postgresql://alaz:alaz@localhost:5435/alaz_test`
//!
//! Override with the `TEST_DATABASE_URL` environment variable.
//!
//! Run with:
//!   cargo test -p alaz-db --test integration
//!
//! Tests marked `#[ignore]` require the full infrastructure (Qdrant, Ollama, TEI)
//! and should only be run locally with `cargo test -p alaz-db --test integration -- --ignored`.

use alaz_core::models::*;
use alaz_db::repos::*;
use alaz_db::{create_pool, run_migrations};
use sqlx::PgPool;

/// Create a pool connected to the test database, run migrations, and truncate
/// all tables for isolation between tests.
async fn setup_test_db() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://alaz:alaz@localhost:5435/alaz_test".to_string());
    let pool = create_pool(&url).await.expect("failed to create pool");
    run_migrations(&pool)
        .await
        .expect("failed to run migrations");
    sqlx::raw_sql(
        "TRUNCATE knowledge_items, episodes, procedures, core_memories, \
         graph_edges, session_logs, reflections, raptor_trees, raptor_nodes, \
         projects, search_queries, vault_secrets, owners CASCADE",
    )
    .execute(&pool)
    .await
    .expect("failed to truncate tables");
    pool
}

// ============================================================
// Project tests
// ============================================================

mod project {
    use super::*;

    #[tokio::test]
    async fn get_or_create_inserts_new_project() {
        let pool = setup_test_db().await;

        let project = ProjectRepo::get_or_create(&pool, "test-project", Some("/tmp/test"))
            .await
            .unwrap();

        assert_eq!(project.name, "test-project");
        assert_eq!(project.path.as_deref(), Some("/tmp/test"));
        assert!(!project.id.is_empty());
    }

    #[tokio::test]
    async fn get_or_create_is_idempotent() {
        let pool = setup_test_db().await;

        let p1 = ProjectRepo::get_or_create(&pool, "test-project", Some("/tmp/a"))
            .await
            .unwrap();
        let p2 = ProjectRepo::get_or_create(&pool, "test-project", Some("/tmp/b"))
            .await
            .unwrap();

        // Same project, same id
        assert_eq!(p1.id, p2.id);
        // Path updated on conflict
        assert_eq!(p2.path.as_deref(), Some("/tmp/b"));
    }

    #[tokio::test]
    async fn get_by_name_returns_none_for_missing() {
        let pool = setup_test_db().await;

        let result = ProjectRepo::get_by_name(&pool, "nonexistent")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_returns_all_projects() {
        let pool = setup_test_db().await;

        ProjectRepo::get_or_create(&pool, "alpha", None)
            .await
            .unwrap();
        ProjectRepo::get_or_create(&pool, "beta", None)
            .await
            .unwrap();

        let projects = ProjectRepo::list(&pool).await.unwrap();
        assert_eq!(projects.len(), 2);
        // Ordered by name ASC
        assert_eq!(projects[0].name, "alpha");
        assert_eq!(projects[1].name, "beta");
    }
}

// ============================================================
// Knowledge CRUD tests
// ============================================================

mod knowledge {
    use super::*;

    fn make_create_input(title: &str, content: &str) -> CreateKnowledge {
        CreateKnowledge {
            title: title.to_string(),
            content: content.to_string(),
            description: Some("A test knowledge item".to_string()),
            kind: Some("artifact".to_string()),
            language: Some("rust".to_string()),
            file_path: Some("src/main.rs".to_string()),
            project: None,
            tags: Some(vec!["test".to_string(), "integration".to_string()]),
            valid_from: None,
            valid_until: None,
            source: None,
            source_metadata: None,
        }
    }

    #[tokio::test]
    async fn create_and_get() {
        let pool = setup_test_db().await;
        let input = make_create_input("Test Item", "fn main() {}");

        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();
        assert_eq!(created.title, "Test Item");
        assert_eq!(created.content, "fn main() {}");
        assert_eq!(created.kind, "artifact");
        assert_eq!(created.language.as_deref(), Some("rust"));
        assert_eq!(created.tags, vec!["test", "integration"]);
        assert_eq!(created.access_count, 0);
        assert!(created.needs_embedding);

        // get() should bump access_count
        let fetched = KnowledgeRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.access_count, 1);
        assert!(fetched.last_accessed_at.is_some());
    }

    #[tokio::test]
    async fn get_bumps_access_count_each_time() {
        let pool = setup_test_db().await;
        let input = make_create_input("Counter Test", "content");
        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();

        for i in 1..=5 {
            let item = KnowledgeRepo::get(&pool, &created.id).await.unwrap();
            assert_eq!(item.access_count, i);
        }
    }

    #[tokio::test]
    async fn get_nonexistent_returns_not_found() {
        let pool = setup_test_db().await;

        let result = KnowledgeRepo::get(&pool, "nonexistent-id").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("not found"),
            "Expected NotFound, got: {err}"
        );
    }

    #[tokio::test]
    async fn update_sets_needs_embedding() {
        let pool = setup_test_db().await;
        let input = make_create_input("Original Title", "original content");
        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();

        // Mark as embedded first
        KnowledgeRepo::mark_embedded(&pool, &created.id)
            .await
            .unwrap();
        let item = KnowledgeRepo::get(&pool, &created.id).await.unwrap();
        assert!(!item.needs_embedding);

        // Now update
        let update = UpdateKnowledge {
            title: Some("Updated Title".to_string()),
            content: None,
            description: None,
            kind: None,
            language: None,
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            superseded_by: None,
        };
        let updated = KnowledgeRepo::update(&pool, &created.id, &update)
            .await
            .unwrap();
        assert_eq!(updated.title, "Updated Title");
        assert!(
            updated.needs_embedding,
            "update must set needs_embedding = true"
        );
        // Content should remain unchanged
        assert_eq!(updated.content, "original content");
    }

    #[tokio::test]
    async fn update_content_preserves_other_fields() {
        let pool = setup_test_db().await;
        let input = make_create_input("Preserve Test", "original");
        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();

        let update = UpdateKnowledge {
            title: None,
            content: Some("updated content".to_string()),
            description: None,
            kind: None,
            language: None,
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            superseded_by: None,
        };
        let updated = KnowledgeRepo::update(&pool, &created.id, &update)
            .await
            .unwrap();
        assert_eq!(updated.title, "Preserve Test");
        assert_eq!(updated.content, "updated content");
        assert_eq!(updated.language.as_deref(), Some("rust"));
        assert_eq!(updated.tags, vec!["test", "integration"]);
    }

    #[tokio::test]
    async fn delete_removes_item() {
        let pool = setup_test_db().await;
        let input = make_create_input("To Delete", "bye");
        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();

        KnowledgeRepo::delete(&pool, &created.id).await.unwrap();

        let result = KnowledgeRepo::get(&pool, &created.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_nonexistent_returns_not_found() {
        let pool = setup_test_db().await;

        let result = KnowledgeRepo::delete(&pool, "nonexistent-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_with_no_filters() {
        let pool = setup_test_db().await;

        for i in 0..3 {
            let input = make_create_input(&format!("Item {i}"), &format!("Content {i}"));
            KnowledgeRepo::create(&pool, &input, None).await.unwrap();
        }

        let filter = ListKnowledgeFilter::default();
        let items = KnowledgeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn list_filters_by_kind() {
        let pool = setup_test_db().await;

        let mut artifact = make_create_input("Artifact", "code");
        artifact.kind = Some("artifact".to_string());
        KnowledgeRepo::create(&pool, &artifact, None).await.unwrap();

        let mut pattern = make_create_input("Pattern", "pattern");
        pattern.kind = Some("pattern".to_string());
        KnowledgeRepo::create(&pool, &pattern, None).await.unwrap();

        let filter = ListKnowledgeFilter {
            kind: Some("pattern".to_string()),
            ..Default::default()
        };
        let items = KnowledgeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Pattern");
    }

    #[tokio::test]
    async fn list_filters_by_language() {
        let pool = setup_test_db().await;

        let mut rust_item = make_create_input("Rust Item", "code");
        rust_item.language = Some("rust".to_string());
        KnowledgeRepo::create(&pool, &rust_item, None)
            .await
            .unwrap();

        let mut python_item = make_create_input("Python Item", "code");
        python_item.language = Some("python".to_string());
        KnowledgeRepo::create(&pool, &python_item, None)
            .await
            .unwrap();

        let filter = ListKnowledgeFilter {
            language: Some("python".to_string()),
            ..Default::default()
        };
        let items = KnowledgeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Python Item");
    }

    #[tokio::test]
    async fn list_filters_by_tag() {
        let pool = setup_test_db().await;

        let mut tagged = make_create_input("Tagged", "content");
        tagged.tags = Some(vec!["special".to_string()]);
        KnowledgeRepo::create(&pool, &tagged, None).await.unwrap();

        let untagged = make_create_input("Untagged", "content");
        KnowledgeRepo::create(&pool, &untagged, None).await.unwrap();

        let filter = ListKnowledgeFilter {
            tag: Some("special".to_string()),
            ..Default::default()
        };
        let items = KnowledgeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Tagged");
    }

    #[tokio::test]
    async fn list_filters_by_project() {
        let pool = setup_test_db().await;

        let project = ProjectRepo::get_or_create(&pool, "proj-a", None)
            .await
            .unwrap();
        let input = make_create_input("Project Item", "code");
        KnowledgeRepo::create(&pool, &input, Some(&project.id))
            .await
            .unwrap();

        let orphan = make_create_input("Orphan Item", "code");
        KnowledgeRepo::create(&pool, &orphan, None).await.unwrap();

        let filter = ListKnowledgeFilter {
            project: Some(project.id.clone()),
            ..Default::default()
        };
        let items = KnowledgeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Project Item");
    }

    #[tokio::test]
    async fn list_respects_limit_and_offset() {
        let pool = setup_test_db().await;

        for i in 0..5 {
            let input = make_create_input(&format!("Item {i}"), &format!("Content {i}"));
            KnowledgeRepo::create(&pool, &input, None).await.unwrap();
        }

        let filter = ListKnowledgeFilter {
            limit: Some(2),
            offset: Some(0),
            ..Default::default()
        };
        let page1 = KnowledgeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(page1.len(), 2);

        let filter2 = ListKnowledgeFilter {
            limit: Some(2),
            offset: Some(2),
            ..Default::default()
        };
        let page2 = KnowledgeRepo::list(&pool, &filter2).await.unwrap();
        assert_eq!(page2.len(), 2);
        // No overlap
        assert_ne!(page1[0].id, page2[0].id);
    }

    #[tokio::test]
    async fn fts_search_finds_matching_items() {
        let pool = setup_test_db().await;

        let input1 = make_create_input(
            "Rust Error Handling",
            "The Result type is used for error handling in Rust programs",
        );
        KnowledgeRepo::create(&pool, &input1, None).await.unwrap();

        let input2 = make_create_input(
            "Python Decorators",
            "Decorators are a powerful feature in Python for modifying functions",
        );
        KnowledgeRepo::create(&pool, &input2, None).await.unwrap();

        // Search for "error handling" should find the Rust item
        let results = KnowledgeRepo::fts_search(&pool, "error handling", None, 10)
            .await
            .unwrap();
        assert!(!results.is_empty(), "FTS search should return results");
        assert_eq!(results[0].1, "Rust Error Handling");

        // Search for "decorators python" should find the Python item
        let results = KnowledgeRepo::fts_search(&pool, "decorators python", None, 10)
            .await
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].1, "Python Decorators");
    }

    #[tokio::test]
    async fn fts_search_returns_empty_for_no_match() {
        let pool = setup_test_db().await;

        let input = make_create_input("Something", "Content about Rust");
        KnowledgeRepo::create(&pool, &input, None).await.unwrap();

        let results = KnowledgeRepo::fts_search(&pool, "quantum entanglement", None, 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fts_search_respects_project_filter() {
        let pool = setup_test_db().await;

        let proj = ProjectRepo::get_or_create(&pool, "search-proj", None)
            .await
            .unwrap();

        let input1 = make_create_input("Database indexing", "B-tree indexes for fast lookups");
        KnowledgeRepo::create(&pool, &input1, Some(&proj.id))
            .await
            .unwrap();

        let input2 = make_create_input("Database sharding", "Horizontal sharding strategies");
        KnowledgeRepo::create(&pool, &input2, None).await.unwrap();

        let results = KnowledgeRepo::fts_search(&pool, "database", Some(&proj.id), 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "Database indexing");
    }

    #[tokio::test]
    async fn find_needing_embedding_returns_new_items() {
        let pool = setup_test_db().await;

        let input = make_create_input("Needs Embedding", "content");
        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();
        assert!(created.needs_embedding);

        let items = KnowledgeRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, created.id);
    }

    #[tokio::test]
    async fn mark_embedded_clears_flag() {
        let pool = setup_test_db().await;

        let input = make_create_input("To Embed", "content");
        let created = KnowledgeRepo::create(&pool, &input, None).await.unwrap();

        KnowledgeRepo::mark_embedded(&pool, &created.id)
            .await
            .unwrap();

        let items = KnowledgeRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn supersede_sets_valid_until_and_superseded_by() {
        let pool = setup_test_db().await;

        let old_input = make_create_input("Old Version", "v1");
        let old = KnowledgeRepo::create(&pool, &old_input, None)
            .await
            .unwrap();

        let new_input = make_create_input("New Version", "v2");
        let new = KnowledgeRepo::create(&pool, &new_input, None)
            .await
            .unwrap();

        KnowledgeRepo::supersede(&pool, &old.id, &new.id, None)
            .await
            .unwrap();

        let old_item = KnowledgeRepo::get(&pool, &old.id).await.unwrap();
        assert_eq!(old_item.superseded_by.as_deref(), Some(new.id.as_str()));
        assert!(old_item.valid_until.is_some());
    }
}

// ============================================================
// Episode CRUD tests
// ============================================================

mod episode {
    use super::*;

    fn make_episode(title: &str) -> CreateEpisode {
        CreateEpisode {
            title: title.to_string(),
            content: "Test episode content".to_string(),
            kind: Some("error".to_string()),
            severity: Some("high".to_string()),
            resolved: Some(false),
            who_cues: Some(vec!["user-alice".to_string()]),
            what_cues: Some(vec!["login-failure".to_string(), "auth-error".to_string()]),
            where_cues: Some(vec!["auth-service".to_string()]),
            when_cues: Some(vec!["2024-01".to_string()]),
            why_cues: Some(vec!["expired-token".to_string()]),
            project: None,
            source: None,
            source_metadata: None,
            action: None,
            outcome: None,
            outcome_score: None,
            related_files: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_episode() {
        let pool = setup_test_db().await;
        let input = make_episode("Auth Error");

        let created = EpisodeRepo::create(&pool, &input, None).await.unwrap();
        assert_eq!(created.title, "Auth Error");
        assert_eq!(created.kind, "error");
        assert_eq!(created.severity.as_deref(), Some("high"));
        assert!(!created.resolved);
        assert_eq!(created.who_cues, vec!["user-alice"]);
        assert_eq!(created.what_cues, vec!["login-failure", "auth-error"]);
        assert!(created.needs_embedding);

        let fetched = EpisodeRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn delete_episode() {
        let pool = setup_test_db().await;
        let input = make_episode("To Delete");
        let created = EpisodeRepo::create(&pool, &input, None).await.unwrap();

        EpisodeRepo::delete(&pool, &created.id).await.unwrap();

        let result = EpisodeRepo::get(&pool, &created.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_episodes_with_filters() {
        let pool = setup_test_db().await;

        let mut error_ep = make_episode("Error Episode");
        error_ep.kind = Some("error".to_string());
        error_ep.resolved = Some(false);
        EpisodeRepo::create(&pool, &error_ep, None).await.unwrap();

        let mut success_ep = make_episode("Success Episode");
        success_ep.kind = Some("success".to_string());
        success_ep.resolved = Some(true);
        EpisodeRepo::create(&pool, &success_ep, None).await.unwrap();

        // Filter by kind
        let filter = ListEpisodesFilter {
            kind: Some("error".to_string()),
            ..Default::default()
        };
        let items = EpisodeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Error Episode");

        // Filter by resolved
        let filter = ListEpisodesFilter {
            resolved: Some(true),
            ..Default::default()
        };
        let items = EpisodeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Success Episode");
    }

    #[tokio::test]
    async fn list_episodes_with_project_filter() {
        let pool = setup_test_db().await;

        let proj = ProjectRepo::get_or_create(&pool, "ep-proj", None)
            .await
            .unwrap();

        let ep1 = make_episode("In Project");
        EpisodeRepo::create(&pool, &ep1, Some(&proj.id))
            .await
            .unwrap();

        let ep2 = make_episode("No Project");
        EpisodeRepo::create(&pool, &ep2, None).await.unwrap();

        let filter = ListEpisodesFilter {
            project: Some(proj.id.clone()),
            ..Default::default()
        };
        let items = EpisodeRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "In Project");
    }

    #[tokio::test]
    async fn cue_search_finds_by_who() {
        let pool = setup_test_db().await;

        let mut ep1 = make_episode("Alice Episode");
        ep1.who_cues = Some(vec!["alice".to_string()]);
        EpisodeRepo::create(&pool, &ep1, None).await.unwrap();

        let mut ep2 = make_episode("Bob Episode");
        ep2.who_cues = Some(vec!["bob".to_string()]);
        EpisodeRepo::create(&pool, &ep2, None).await.unwrap();

        let results = EpisodeRepo::cue_search(
            &pool,
            Some(&["alice".to_string()]),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Alice Episode");
    }

    #[tokio::test]
    async fn cue_search_finds_by_what() {
        let pool = setup_test_db().await;

        let mut ep1 = make_episode("Auth Episode");
        ep1.what_cues = Some(vec!["authentication".to_string()]);
        EpisodeRepo::create(&pool, &ep1, None).await.unwrap();

        let mut ep2 = make_episode("DB Episode");
        ep2.what_cues = Some(vec!["database".to_string()]);
        EpisodeRepo::create(&pool, &ep2, None).await.unwrap();

        let results = EpisodeRepo::cue_search(
            &pool,
            None,
            Some(&["authentication".to_string()]),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Auth Episode");
    }

    #[tokio::test]
    async fn cue_search_with_multiple_cues() {
        let pool = setup_test_db().await;

        let mut ep = make_episode("Multi-cue");
        ep.who_cues = Some(vec!["alice".to_string()]);
        ep.what_cues = Some(vec!["deploy".to_string()]);
        ep.where_cues = Some(vec!["production".to_string()]);
        EpisodeRepo::create(&pool, &ep, None).await.unwrap();

        let mut other = make_episode("Other");
        other.who_cues = Some(vec!["bob".to_string()]);
        other.what_cues = Some(vec!["deploy".to_string()]);
        other.where_cues = Some(vec!["staging".to_string()]);
        EpisodeRepo::create(&pool, &other, None).await.unwrap();

        // Search with who=alice AND where=production
        let results = EpisodeRepo::cue_search(
            &pool,
            Some(&["alice".to_string()]),
            None,
            Some(&["production".to_string()]),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Multi-cue");
    }

    #[tokio::test]
    async fn cue_search_with_empty_cues_returns_all() {
        let pool = setup_test_db().await;

        EpisodeRepo::create(&pool, &make_episode("Ep1"), None)
            .await
            .unwrap();
        EpisodeRepo::create(&pool, &make_episode("Ep2"), None)
            .await
            .unwrap();

        // All empty cues => no filtering => return all
        let results = EpisodeRepo::cue_search(&pool, None, None, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn find_needing_embedding_and_mark() {
        let pool = setup_test_db().await;

        let ep = make_episode("Embed Me");
        let created = EpisodeRepo::create(&pool, &ep, None).await.unwrap();

        let needing = EpisodeRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert_eq!(needing.len(), 1);

        EpisodeRepo::mark_embedded(&pool, &created.id)
            .await
            .unwrap();

        let needing = EpisodeRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert!(needing.is_empty());
    }
}

// ============================================================
// Procedure CRUD tests
// ============================================================

mod procedure {
    use super::*;

    fn make_procedure(title: &str) -> CreateProcedure {
        CreateProcedure {
            title: title.to_string(),
            content: "Step-by-step instructions".to_string(),
            steps: Some(serde_json::json!(["step 1", "step 2", "step 3"])),
            project: None,
            tags: Some(vec!["deployment".to_string()]),
            source: None,
            source_metadata: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_procedure() {
        let pool = setup_test_db().await;
        let input = make_procedure("Deploy Procedure");

        let created = ProcedureRepo::create(&pool, &input, None).await.unwrap();
        assert_eq!(created.title, "Deploy Procedure");
        assert_eq!(created.content, "Step-by-step instructions");
        assert_eq!(created.times_used, 0);
        assert_eq!(created.success, 0);
        assert_eq!(created.failure, 0);
        assert!(created.success_rate.is_none()); // 0/0 => NULL
        assert_eq!(created.tags, vec!["deployment"]);
        assert!(created.needs_embedding);

        let fetched = ProcedureRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn delete_procedure() {
        let pool = setup_test_db().await;
        let input = make_procedure("To Delete");
        let created = ProcedureRepo::create(&pool, &input, None).await.unwrap();

        ProcedureRepo::delete(&pool, &created.id).await.unwrap();

        let result = ProcedureRepo::get(&pool, &created.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn record_outcome_success() {
        let pool = setup_test_db().await;
        let input = make_procedure("Success Proc");
        let created = ProcedureRepo::create(&pool, &input, None).await.unwrap();

        ProcedureRepo::record_outcome(&pool, &created.id, true)
            .await
            .unwrap();

        let proc = ProcedureRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(proc.times_used, 1);
        assert_eq!(proc.success, 1);
        assert_eq!(proc.failure, 0);
        // Wilson score for 1/1 ≈ 0.2065 (penalizes small samples)
        assert!(
            (proc.success_rate.unwrap() - 0.2065).abs() < 0.01,
            "Wilson score for 1/1 should be ~0.2065, got {}",
            proc.success_rate.unwrap()
        );
    }

    #[tokio::test]
    async fn record_outcome_failure() {
        let pool = setup_test_db().await;
        let input = make_procedure("Failure Proc");
        let created = ProcedureRepo::create(&pool, &input, None).await.unwrap();

        ProcedureRepo::record_outcome(&pool, &created.id, false)
            .await
            .unwrap();

        let proc = ProcedureRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(proc.times_used, 1);
        assert_eq!(proc.success, 0);
        assert_eq!(proc.failure, 1);
        // Wilson score for 0/1 = 0.0
        assert!(
            (proc.success_rate.unwrap() - 0.0).abs() < 0.01,
            "Wilson score for 0/1 should be ~0.0, got {}",
            proc.success_rate.unwrap()
        );
    }

    #[tokio::test]
    async fn record_outcome_mixed() {
        let pool = setup_test_db().await;
        let input = make_procedure("Mixed Proc");
        let created = ProcedureRepo::create(&pool, &input, None).await.unwrap();

        ProcedureRepo::record_outcome(&pool, &created.id, true)
            .await
            .unwrap();
        ProcedureRepo::record_outcome(&pool, &created.id, true)
            .await
            .unwrap();
        ProcedureRepo::record_outcome(&pool, &created.id, false)
            .await
            .unwrap();

        let proc = ProcedureRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(proc.times_used, 3);
        assert_eq!(proc.success, 2);
        assert_eq!(proc.failure, 1);
        // Wilson score for 2/3 ≈ 0.2077 (conservative for small sample)
        assert!(
            (proc.success_rate.unwrap() - 0.2077).abs() < 0.01,
            "Wilson score for 2/3 should be ~0.2077, got {}",
            proc.success_rate.unwrap()
        );
    }

    #[tokio::test]
    async fn record_outcome_nonexistent_returns_not_found() {
        let pool = setup_test_db().await;

        let result = ProcedureRepo::record_outcome(&pool, "nonexistent", true).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_filters_by_tag() {
        let pool = setup_test_db().await;

        let mut p1 = make_procedure("Tagged Proc");
        p1.tags = Some(vec!["ci".to_string()]);
        ProcedureRepo::create(&pool, &p1, None).await.unwrap();

        let mut p2 = make_procedure("Other Proc");
        p2.tags = Some(vec!["manual".to_string()]);
        ProcedureRepo::create(&pool, &p2, None).await.unwrap();

        let filter = ListProceduresFilter {
            tag: Some("ci".to_string()),
            ..Default::default()
        };
        let items = ProcedureRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Tagged Proc");
    }

    #[tokio::test]
    async fn list_filters_by_project() {
        let pool = setup_test_db().await;
        let proj = ProjectRepo::get_or_create(&pool, "proc-proj", None)
            .await
            .unwrap();

        let p1 = make_procedure("In Project");
        ProcedureRepo::create(&pool, &p1, Some(&proj.id))
            .await
            .unwrap();

        let p2 = make_procedure("No Project");
        ProcedureRepo::create(&pool, &p2, None).await.unwrap();

        let filter = ListProceduresFilter {
            project: Some(proj.id.clone()),
            ..Default::default()
        };
        let items = ProcedureRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "In Project");
    }

    #[tokio::test]
    async fn find_needing_embedding_and_mark() {
        let pool = setup_test_db().await;
        let p = make_procedure("Embed Me");
        let created = ProcedureRepo::create(&pool, &p, None).await.unwrap();

        let needing = ProcedureRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert_eq!(needing.len(), 1);

        ProcedureRepo::mark_embedded(&pool, &created.id)
            .await
            .unwrap();

        let needing = ProcedureRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert!(needing.is_empty());
    }
}

// ============================================================
// Core memory tests
// ============================================================

mod core_memory {
    use super::*;

    fn make_memory(category: &str, key: &str, value: &str) -> UpsertCoreMemory {
        UpsertCoreMemory {
            category: category.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            confidence: Some(0.9),
            project: None,
        }
    }

    #[tokio::test]
    async fn upsert_creates_new_memory() {
        let pool = setup_test_db().await;
        let input = make_memory("preference", "editor", "vim");

        let created = CoreMemoryRepo::upsert(&pool, &input, None).await.unwrap();
        assert_eq!(created.category, "preference");
        assert_eq!(created.key, "editor");
        assert_eq!(created.value, "vim");
        assert!((created.confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(created.confirmations, 1);
        assert_eq!(created.contradictions, 0);
    }

    #[tokio::test]
    async fn upsert_updates_existing_memory() {
        let pool = setup_test_db().await;

        let input1 = make_memory("preference", "editor", "vim");
        let first = CoreMemoryRepo::upsert(&pool, &input1, None).await.unwrap();

        // Upsert with same category+key => update
        let input2 = make_memory("preference", "editor", "neovim");
        let second = CoreMemoryRepo::upsert(&pool, &input2, None).await.unwrap();

        assert_eq!(first.id, second.id, "Should reuse same id on conflict");
        assert_eq!(second.value, "neovim");
        assert_eq!(second.confirmations, 2); // Incremented on conflict
    }

    #[tokio::test]
    async fn upsert_with_project_scoping() {
        let pool = setup_test_db().await;

        let proj = ProjectRepo::get_or_create(&pool, "mem-proj", None)
            .await
            .unwrap();

        // Same key, no project
        let global = make_memory("preference", "lang", "rust");
        let g = CoreMemoryRepo::upsert(&pool, &global, None).await.unwrap();

        // Same key, with project
        let scoped = make_memory("preference", "lang", "python");
        let s = CoreMemoryRepo::upsert(&pool, &scoped, Some(&proj.id))
            .await
            .unwrap();

        // They should be different records
        assert_ne!(g.id, s.id);
        assert_eq!(g.value, "rust");
        assert_eq!(s.value, "python");
    }

    #[tokio::test]
    async fn get_core_memory() {
        let pool = setup_test_db().await;
        let input = make_memory("fact", "os", "linux");
        let created = CoreMemoryRepo::upsert(&pool, &input, None).await.unwrap();

        let fetched = CoreMemoryRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.value, "linux");
    }

    #[tokio::test]
    async fn delete_core_memory() {
        let pool = setup_test_db().await;
        let input = make_memory("fact", "delete-me", "bye");
        let created = CoreMemoryRepo::upsert(&pool, &input, None).await.unwrap();

        CoreMemoryRepo::delete(&pool, &created.id).await.unwrap();

        let result = CoreMemoryRepo::get(&pool, &created.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn record_contradiction_increments_counter() {
        let pool = setup_test_db().await;
        let input = make_memory("fact", "color", "blue");
        let created = CoreMemoryRepo::upsert(&pool, &input, None).await.unwrap();
        assert_eq!(created.contradictions, 0);

        CoreMemoryRepo::record_contradiction(&pool, &created.id)
            .await
            .unwrap();
        CoreMemoryRepo::record_contradiction(&pool, &created.id)
            .await
            .unwrap();

        let fetched = CoreMemoryRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.contradictions, 2);
    }

    #[tokio::test]
    async fn list_filters_by_category() {
        let pool = setup_test_db().await;

        CoreMemoryRepo::upsert(&pool, &make_memory("preference", "k1", "v1"), None)
            .await
            .unwrap();
        CoreMemoryRepo::upsert(&pool, &make_memory("fact", "k2", "v2"), None)
            .await
            .unwrap();
        CoreMemoryRepo::upsert(&pool, &make_memory("preference", "k3", "v3"), None)
            .await
            .unwrap();

        let filter = ListCoreMemoryFilter {
            category: Some("preference".to_string()),
            ..Default::default()
        };
        let items = CoreMemoryRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|m| m.category == "preference"));
    }

    #[tokio::test]
    async fn list_filters_by_project() {
        let pool = setup_test_db().await;
        let proj = ProjectRepo::get_or_create(&pool, "cm-proj", None)
            .await
            .unwrap();

        CoreMemoryRepo::upsert(&pool, &make_memory("fact", "a", "1"), Some(&proj.id))
            .await
            .unwrap();
        CoreMemoryRepo::upsert(&pool, &make_memory("fact", "b", "2"), None)
            .await
            .unwrap();

        let filter = ListCoreMemoryFilter {
            project: Some(proj.id.clone()),
            ..Default::default()
        };
        let items = CoreMemoryRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "a");
    }

    #[tokio::test]
    async fn upsert_default_confidence_is_one() {
        let pool = setup_test_db().await;

        let input = UpsertCoreMemory {
            category: "fact".to_string(),
            key: "default-conf".to_string(),
            value: "test".to_string(),
            confidence: None,
            project: None,
        };
        let mem = CoreMemoryRepo::upsert(&pool, &input, None).await.unwrap();
        assert!((mem.confidence - 1.0).abs() < f64::EPSILON);
    }
}

// ============================================================
// Graph edge tests
// ============================================================

mod graph {
    use super::*;

    fn make_edge(source_id: &str, target_id: &str, relation: &str) -> CreateRelation {
        CreateRelation {
            source_type: "knowledge".to_string(),
            source_id: source_id.to_string(),
            target_type: "knowledge".to_string(),
            target_id: target_id.to_string(),
            relation: relation.to_string(),
            weight: Some(1.0),
            description: Some("Test edge".to_string()),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_edge() {
        let pool = setup_test_db().await;
        let input = make_edge("item-a", "item-b", "relates_to");

        let created = GraphRepo::create_edge(&pool, &input).await.unwrap();
        assert_eq!(created.source_type, "knowledge");
        assert_eq!(created.source_id, "item-a");
        assert_eq!(created.target_type, "knowledge");
        assert_eq!(created.target_id, "item-b");
        assert_eq!(created.relation, "relates_to");
        assert!((created.weight - 1.0).abs() < f64::EPSILON);
        assert_eq!(created.usage_count, 1);

        let fetched = GraphRepo::get_edge(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn create_duplicate_edge_increments_usage_count() {
        let pool = setup_test_db().await;

        let input = make_edge("item-a", "item-b", "relates_to");
        let first = GraphRepo::create_edge(&pool, &input).await.unwrap();
        let second = GraphRepo::create_edge(&pool, &input).await.unwrap();

        // Same edge (by unique constraint), usage_count bumped
        assert_eq!(first.id, second.id);
        assert_eq!(second.usage_count, 2);
    }

    #[tokio::test]
    async fn delete_edge() {
        let pool = setup_test_db().await;
        let input = make_edge("item-a", "item-b", "relates_to");
        let created = GraphRepo::create_edge(&pool, &input).await.unwrap();

        GraphRepo::delete_edge(&pool, &created.id).await.unwrap();

        let result = GraphRepo::get_edge(&pool, &created.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_edges_outgoing() {
        let pool = setup_test_db().await;

        let e1 = make_edge("item-a", "item-b", "depends_on");
        GraphRepo::create_edge(&pool, &e1).await.unwrap();

        let e2 = make_edge("item-a", "item-c", "relates_to");
        GraphRepo::create_edge(&pool, &e2).await.unwrap();

        // Unrelated edge
        let e3 = make_edge("item-x", "item-y", "relates_to");
        GraphRepo::create_edge(&pool, &e3).await.unwrap();

        let edges = GraphRepo::get_edges(&pool, "knowledge", "item-a", "outgoing")
            .await
            .unwrap();
        assert_eq!(edges.len(), 2);
        assert!(edges.iter().all(|e| e.source_id == "item-a"));
    }

    #[tokio::test]
    async fn get_edges_incoming() {
        let pool = setup_test_db().await;

        let e1 = make_edge("item-a", "item-b", "depends_on");
        GraphRepo::create_edge(&pool, &e1).await.unwrap();

        let e2 = make_edge("item-c", "item-b", "relates_to");
        GraphRepo::create_edge(&pool, &e2).await.unwrap();

        let edges = GraphRepo::get_edges(&pool, "knowledge", "item-b", "incoming")
            .await
            .unwrap();
        assert_eq!(edges.len(), 2);
        assert!(edges.iter().all(|e| e.target_id == "item-b"));
    }

    #[tokio::test]
    async fn get_edges_both_directions() {
        let pool = setup_test_db().await;

        let e1 = make_edge("item-a", "item-b", "depends_on");
        GraphRepo::create_edge(&pool, &e1).await.unwrap();

        let e2 = make_edge("item-c", "item-a", "relates_to");
        GraphRepo::create_edge(&pool, &e2).await.unwrap();

        let edges = GraphRepo::get_edges(&pool, "knowledge", "item-a", "both")
            .await
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[tokio::test]
    async fn get_edges_returns_empty_for_unknown_entity() {
        let pool = setup_test_db().await;

        let edges = GraphRepo::get_edges(&pool, "knowledge", "nonexistent", "both")
            .await
            .unwrap();
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn decay_weights_reduces_weight() {
        let pool = setup_test_db().await;

        let input = make_edge("item-a", "item-b", "relates_to");
        let created = GraphRepo::create_edge(&pool, &input).await.unwrap();
        let original_weight = created.weight;

        GraphRepo::decay_weights(&pool).await.unwrap();

        let decayed = GraphRepo::get_edge(&pool, &created.id).await.unwrap();
        assert!(
            decayed.weight < original_weight,
            "Weight should decrease after decay: {} < {}",
            decayed.weight,
            original_weight
        );
        // 1.0 * 0.95 = 0.95
        assert!((decayed.weight - 0.95).abs() < 0.001);
    }

    #[tokio::test]
    async fn decay_weights_deletes_below_threshold() {
        let pool = setup_test_db().await;

        // Create an edge with very low weight
        let mut input = make_edge("item-a", "item-b", "weak_link");
        input.weight = Some(0.04);
        GraphRepo::create_edge(&pool, &input).await.unwrap();

        // Create another with normal weight
        let strong = make_edge("item-c", "item-d", "strong_link");
        GraphRepo::create_edge(&pool, &strong).await.unwrap();

        // After decay: 0.04 * 0.95 = 0.038 < 0.05 => deleted
        let deleted = GraphRepo::decay_weights(&pool).await.unwrap();
        assert_eq!(deleted, 1);

        // Only the strong edge should remain
        let all_edges = GraphRepo::get_edges(&pool, "knowledge", "item-c", "outgoing")
            .await
            .unwrap();
        assert_eq!(all_edges.len(), 1);
    }

    #[tokio::test]
    async fn create_edge_with_metadata() {
        let pool = setup_test_db().await;

        let input = CreateRelation {
            source_type: "episode".to_string(),
            source_id: "ep-1".to_string(),
            target_type: "knowledge".to_string(),
            target_id: "k-1".to_string(),
            relation: "caused_by".to_string(),
            weight: Some(0.8),
            description: Some("The episode was caused by this knowledge".to_string()),
            metadata: Some(serde_json::json!({"confidence": 0.9, "source": "automated"})),
        };
        let edge = GraphRepo::create_edge(&pool, &input).await.unwrap();
        assert_eq!(edge.source_type, "episode");
        assert_eq!(edge.target_type, "knowledge");
        assert!((edge.weight - 0.8).abs() < f64::EPSILON);
        assert_eq!(edge.metadata["confidence"], 0.9);
    }

    #[tokio::test]
    async fn create_edge_default_weight_is_one() {
        let pool = setup_test_db().await;

        let input = CreateRelation {
            source_type: "knowledge".to_string(),
            source_id: "a".to_string(),
            target_type: "knowledge".to_string(),
            target_id: "b".to_string(),
            relation: "test".to_string(),
            weight: None,
            description: None,
            metadata: None,
        };
        let edge = GraphRepo::create_edge(&pool, &input).await.unwrap();
        assert!((edge.weight - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn edges_ordered_by_weight_desc() {
        let pool = setup_test_db().await;

        let mut low = make_edge("node", "low", "link");
        low.weight = Some(0.3);
        GraphRepo::create_edge(&pool, &low).await.unwrap();

        let mut high = make_edge("node", "high", "link");
        high.weight = Some(0.9);
        GraphRepo::create_edge(&pool, &high).await.unwrap();

        let mut mid = make_edge("node", "mid", "link");
        mid.weight = Some(0.6);
        GraphRepo::create_edge(&pool, &mid).await.unwrap();

        let edges = GraphRepo::get_edges(&pool, "knowledge", "node", "outgoing")
            .await
            .unwrap();
        assert_eq!(edges.len(), 3);
        assert!(edges[0].weight >= edges[1].weight);
        assert!(edges[1].weight >= edges[2].weight);
    }
}

// ============================================================
// Cross-entity integration tests
// ============================================================

mod cross_entity {
    use super::*;

    #[tokio::test]
    async fn knowledge_with_project_and_graph_edges() {
        let pool = setup_test_db().await;

        // Create a project
        let proj = ProjectRepo::get_or_create(&pool, "cross-test", None)
            .await
            .unwrap();

        // Create two knowledge items in the project
        let k1_input = CreateKnowledge {
            title: "Auth Module".to_string(),
            content: "Authentication module code".to_string(),
            description: None,
            kind: Some("artifact".to_string()),
            language: Some("rust".to_string()),
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            source: None,
            source_metadata: None,
        };
        let k1 = KnowledgeRepo::create(&pool, &k1_input, Some(&proj.id))
            .await
            .unwrap();

        let k2_input = CreateKnowledge {
            title: "Auth Config".to_string(),
            content: "Configuration for auth module".to_string(),
            description: None,
            kind: Some("artifact".to_string()),
            language: Some("toml".to_string()),
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            source: None,
            source_metadata: None,
        };
        let k2 = KnowledgeRepo::create(&pool, &k2_input, Some(&proj.id))
            .await
            .unwrap();

        // Create a graph edge between them
        let edge_input = CreateRelation {
            source_type: "knowledge".to_string(),
            source_id: k1.id.clone(),
            target_type: "knowledge".to_string(),
            target_id: k2.id.clone(),
            relation: "configures".to_string(),
            weight: Some(0.9),
            description: Some("Auth module is configured by auth config".to_string()),
            metadata: None,
        };
        let edge = GraphRepo::create_edge(&pool, &edge_input).await.unwrap();

        // Verify traversal from k1 finds k2
        let outgoing = GraphRepo::get_edges(&pool, "knowledge", &k1.id, "outgoing")
            .await
            .unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].target_id, k2.id);
        assert_eq!(outgoing[0].relation, "configures");

        // Verify reverse traversal from k2 finds k1
        let incoming = GraphRepo::get_edges(&pool, "knowledge", &k2.id, "incoming")
            .await
            .unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source_id, k1.id);

        // Cleanup: deleting edge should not affect knowledge items
        GraphRepo::delete_edge(&pool, &edge.id).await.unwrap();
        let k1_still = KnowledgeRepo::get(&pool, &k1.id).await.unwrap();
        assert_eq!(k1_still.title, "Auth Module");
    }

    #[tokio::test]
    async fn episode_linked_to_knowledge_via_graph() {
        let pool = setup_test_db().await;

        // Create a knowledge item
        let k_input = CreateKnowledge {
            title: "Database Connection Pool".to_string(),
            content: "Connection pooling implementation".to_string(),
            description: None,
            kind: Some("artifact".to_string()),
            language: None,
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            source: None,
            source_metadata: None,
        };
        let knowledge = KnowledgeRepo::create(&pool, &k_input, None).await.unwrap();

        // Create an episode
        let ep_input = CreateEpisode {
            title: "Connection Pool Exhaustion".to_string(),
            content: "All connections were exhausted under load".to_string(),
            kind: Some("error".to_string()),
            severity: Some("critical".to_string()),
            resolved: Some(false),
            who_cues: None,
            what_cues: Some(vec!["pool-exhaustion".to_string()]),
            where_cues: Some(vec!["database".to_string()]),
            when_cues: None,
            why_cues: None,
            project: None,
            source: None,
            source_metadata: None,
            action: None,
            outcome: None,
            outcome_score: None,
            related_files: None,
        };
        let episode = EpisodeRepo::create(&pool, &ep_input, None).await.unwrap();

        // Link the episode to the knowledge via graph
        let edge_input = CreateRelation {
            source_type: "episode".to_string(),
            source_id: episode.id.clone(),
            target_type: "knowledge".to_string(),
            target_id: knowledge.id.clone(),
            relation: "refers_to".to_string(),
            weight: Some(0.8),
            description: None,
            metadata: None,
        };
        GraphRepo::create_edge(&pool, &edge_input).await.unwrap();

        // Traverse from episode to find related knowledge
        let edges = GraphRepo::get_edges(&pool, "episode", &episode.id, "outgoing")
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target_type, "knowledge");
        assert_eq!(edges[0].target_id, knowledge.id);
    }

    #[tokio::test]
    async fn full_lifecycle_create_search_update_delete() {
        let pool = setup_test_db().await;

        // 1. Create
        let input = CreateKnowledge {
            title: "Lifecycle Test Item".to_string(),
            content: "Content about testing lifecycle management patterns".to_string(),
            description: Some("Testing the full CRUD lifecycle".to_string()),
            kind: Some("pattern".to_string()),
            language: Some("rust".to_string()),
            file_path: None,
            project: None,
            tags: Some(vec!["lifecycle".to_string()]),
            valid_from: None,
            valid_until: None,
            source: None,
            source_metadata: None,
        };
        let item = KnowledgeRepo::create(&pool, &input, None).await.unwrap();
        assert!(item.needs_embedding);

        // 2. FTS search finds it
        let results = KnowledgeRepo::fts_search(&pool, "lifecycle management", None, 10)
            .await
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, item.id);

        // 3. Get bumps access
        let fetched = KnowledgeRepo::get(&pool, &item.id).await.unwrap();
        assert_eq!(fetched.access_count, 1);

        // 4. Mark embedded
        KnowledgeRepo::mark_embedded(&pool, &item.id).await.unwrap();
        let embedded_check = KnowledgeRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert!(embedded_check.is_empty());

        // 5. Update re-triggers embedding
        let update = UpdateKnowledge {
            title: None,
            content: Some("Updated content about lifecycle patterns".to_string()),
            description: None,
            kind: None,
            language: None,
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            superseded_by: None,
        };
        let updated = KnowledgeRepo::update(&pool, &item.id, &update)
            .await
            .unwrap();
        assert!(updated.needs_embedding);

        // 6. Delete
        KnowledgeRepo::delete(&pool, &item.id).await.unwrap();
        let result = KnowledgeRepo::get(&pool, &item.id).await;
        assert!(result.is_err());
    }
}

// ============================================================
// Wilson score integration tests (PR: 009_wilson_score)
// ============================================================

mod wilson_score {
    use super::*;

    fn make_procedure(title: &str) -> CreateProcedure {
        CreateProcedure {
            title: title.to_string(),
            content: "Wilson score test procedure".to_string(),
            steps: None,
            project: None,
            tags: None,
            source: None,
            source_metadata: None,
        }
    }

    /// Record `successes` successful outcomes followed by `failures` failures.
    async fn record_outcomes(pool: &PgPool, id: &str, successes: u32, failures: u32) {
        for _ in 0..successes {
            ProcedureRepo::record_outcome(pool, id, true).await.unwrap();
        }
        for _ in 0..failures {
            ProcedureRepo::record_outcome(pool, id, false)
                .await
                .unwrap();
        }
    }

    // ── DB-level Wilson score values ─────────────────────────────────────────

    #[tokio::test]
    async fn new_procedure_has_null_success_rate() {
        let pool = setup_test_db().await;
        let p = ProcedureRepo::create(&pool, &make_procedure("Untested"), None)
            .await
            .unwrap();
        // 0 runs → NULL (not NaN, not 0)
        assert!(
            p.success_rate.is_none(),
            "fresh procedure should have NULL success_rate, got {:?}",
            p.success_rate
        );
    }

    #[tokio::test]
    async fn wilson_score_for_all_success_small_sample() {
        let pool = setup_test_db().await;
        let p = ProcedureRepo::create(&pool, &make_procedure("AllSuccessSmall"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p.id, 1, 0).await;

        let p = ProcedureRepo::get(&pool, &p.id).await.unwrap();
        let score = p.success_rate.unwrap();
        // Wilson(1/1) ≈ 0.2065 — well below naive 1.0
        assert!(
            (score - 0.2065).abs() < 0.01,
            "Wilson(1/1) should be ≈0.2065, got {score}"
        );
    }

    #[tokio::test]
    async fn wilson_score_for_zero_successes() {
        let pool = setup_test_db().await;
        let p = ProcedureRepo::create(&pool, &make_procedure("AllFail"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p.id, 0, 1).await;

        let p = ProcedureRepo::get(&pool, &p.id).await.unwrap();
        let score = p.success_rate.unwrap();
        // Wilson(0/1) = 0.0
        assert!(score.abs() < 0.01, "Wilson(0/1) should be 0.0, got {score}");
    }

    #[tokio::test]
    async fn wilson_score_increases_with_consistent_success() {
        let pool = setup_test_db().await;
        let p = ProcedureRepo::create(&pool, &make_procedure("Growing"), None)
            .await
            .unwrap();

        let mut prev_score = 0.0_f64;
        for i in 1..=10_u32 {
            record_outcomes(&pool, &p.id, 1, 0).await;
            let p_updated = ProcedureRepo::get(&pool, &p.id).await.unwrap();
            let score = p_updated.success_rate.unwrap();
            assert!(
                score > prev_score,
                "After {i} successes: {score} should be > previous {prev_score}"
            );
            prev_score = score;
        }
    }

    #[tokio::test]
    async fn wilson_score_proven_threshold_boundary() {
        let pool = setup_test_db().await;

        // 5/5 → Wilson ≈ 0.566 (above 0.3 threshold) ✓
        let proven = ProcedureRepo::create(&pool, &make_procedure("Proven"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &proven.id, 5, 0).await;
        let proven = ProcedureRepo::get(&pool, &proven.id).await.unwrap();
        let proven_score = proven.success_rate.unwrap();
        assert!(
            proven_score > 0.3,
            "5/5 Wilson={proven_score} should exceed proven threshold 0.3"
        );

        // 1/1 → Wilson ≈ 0.207 (below 0.3 threshold) ✗
        let unproven = ProcedureRepo::create(&pool, &make_procedure("Unproven"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &unproven.id, 1, 0).await;
        let unproven = ProcedureRepo::get(&pool, &unproven.id).await.unwrap();
        let unproven_score = unproven.success_rate.unwrap();
        assert!(
            unproven_score <= 0.3,
            "1/1 Wilson={unproven_score} should be below proven threshold 0.3"
        );
    }

    #[tokio::test]
    async fn wilson_score_large_sample_approaches_naive_rate() {
        let pool = setup_test_db().await;
        let p = ProcedureRepo::create(&pool, &make_procedure("LargeSample"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p.id, 95, 5).await; // 95% naive success

        let p = ProcedureRepo::get(&pool, &p.id).await.unwrap();
        let score = p.success_rate.unwrap();
        // Wilson(95/100) ≈ 0.883 — converges towards naive 0.95
        assert!(score > 0.85, "Wilson(95/100) should be > 0.85, got {score}");
        assert!(
            score < 0.95,
            "Wilson(95/100) should remain below naive rate 0.95, got {score}"
        );
    }

    #[tokio::test]
    async fn wilson_score_high_evidence_outranks_low_evidence() {
        // Core property: more evidence at same naive rate → higher Wilson score
        let pool = setup_test_db().await;

        let p1 = ProcedureRepo::create(&pool, &make_procedure("OneRun"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p1.id, 1, 0).await;
        let p1 = ProcedureRepo::get(&pool, &p1.id).await.unwrap();

        let p2 = ProcedureRepo::create(&pool, &make_procedure("TenRuns"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p2.id, 10, 0).await;
        let p2 = ProcedureRepo::get(&pool, &p2.id).await.unwrap();

        let score1 = p1.success_rate.unwrap();
        let score2 = p2.success_rate.unwrap();
        assert!(
            score2 > score1,
            "10/10 Wilson={score2} should beat 1/1 Wilson={score1}"
        );
    }

    #[tokio::test]
    async fn wilson_scores_reflect_confidence_ordering() {
        // Verify that procedures with more evidence have higher Wilson scores,
        // even when the list itself is not ordered by success_rate.
        let pool = setup_test_db().await;

        let p_low = ProcedureRepo::create(&pool, &make_procedure("LowConf"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p_low.id, 1, 0).await; // Wilson ≈ 0.207

        let p_high = ProcedureRepo::create(&pool, &make_procedure("HighConf"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p_high.id, 10, 0).await; // Wilson ≈ 0.722

        let p_mid = ProcedureRepo::create(&pool, &make_procedure("MidConf"), None)
            .await
            .unwrap();
        record_outcomes(&pool, &p_mid.id, 5, 0).await; // Wilson ≈ 0.566

        let filter = ListProceduresFilter::default();
        let items = ProcedureRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 3);

        // Fetch individual scores by title (list ordered by updated_at DESC)
        let by_title = |title: &str| {
            items
                .iter()
                .find(|p| p.title == title)
                .unwrap()
                .success_rate
                .unwrap()
        };

        let s_low = by_title("LowConf");
        let s_mid = by_title("MidConf");
        let s_high = by_title("HighConf");

        // All scores must be in [0, 1]
        for (name, score) in &[("LowConf", s_low), ("MidConf", s_mid), ("HighConf", s_high)] {
            assert!(
                (0.0..=1.0).contains(score),
                "{name} Wilson score {score} out of [0,1]"
            );
        }

        // Monotone: more evidence at 100% success → higher Wilson score
        assert!(s_low < s_mid, "1/1 ({s_low}) should be < 5/5 ({s_mid})");
        assert!(s_mid < s_high, "5/5 ({s_mid}) should be < 10/10 ({s_high})");

        // Proven threshold (>0.3): only mid and high qualify
        assert!(
            s_low <= 0.3,
            "1/1 Wilson={s_low} should not pass 0.3 threshold"
        );
        assert!(s_mid > 0.3, "5/5 Wilson={s_mid} should pass 0.3 threshold");
        assert!(
            s_high > 0.3,
            "10/10 Wilson={s_high} should pass 0.3 threshold"
        );
    }
}

// ============================================================
// Session tests
// ============================================================

mod session {
    use super::*;

    #[tokio::test]
    async fn test_session_create_and_get() {
        let pool = setup_test_db().await;

        let created = SessionRepo::create(&pool, None).await.unwrap();
        assert!(!created.id.is_empty());
        assert!(created.project_id.is_none());
        assert!(created.cost.is_none());
        assert!(created.input_tokens.is_none());
        assert!(created.output_tokens.is_none());
        assert!(created.duration_seconds.is_none());
        assert!(created.summary.is_none());

        let fetched = SessionRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.project_id, created.project_id);
    }

    #[tokio::test]
    async fn test_session_create_with_project() {
        let pool = setup_test_db().await;
        let proj = ProjectRepo::get_or_create(&pool, "session-proj", None)
            .await
            .unwrap();

        let session = SessionRepo::create(&pool, Some(&proj.id)).await.unwrap();
        assert_eq!(session.project_id.as_deref(), Some(proj.id.as_str()));
    }

    #[tokio::test]
    async fn test_session_ensure_exists_idempotent() {
        let pool = setup_test_db().await;

        let custom_id = "custom-session-001";
        let first = SessionRepo::ensure_exists(&pool, custom_id, None)
            .await
            .unwrap();
        assert_eq!(first.id, custom_id);
        assert_eq!(first.status.as_deref(), Some("started"));

        let second = SessionRepo::ensure_exists(&pool, custom_id, None)
            .await
            .unwrap();
        assert_eq!(second.id, first.id);
        // updated_at should be refreshed but it's the same session
        assert_eq!(second.status.as_deref(), Some("started"));
    }

    #[tokio::test]
    async fn test_session_checkpoint_lifecycle() {
        let pool = setup_test_db().await;

        // Create a session first (FK requirement)
        let session = SessionRepo::create(&pool, None).await.unwrap();

        // Save two checkpoints
        let data1 = serde_json::json!({"task": "implement feature A", "step": 1});
        let cp1 = SessionRepo::save_checkpoint(&pool, &session.id, &data1)
            .await
            .unwrap();
        assert_eq!(cp1.session_id, session.id);
        assert_eq!(cp1.checkpoint_data, data1);

        let data2 =
            serde_json::json!({"task": "implement feature A", "step": 2, "progress": "50%"});
        let cp2 = SessionRepo::save_checkpoint(&pool, &session.id, &data2)
            .await
            .unwrap();
        assert_ne!(cp1.id, cp2.id);

        // get_checkpoints returns all, ordered DESC
        let all = SessionRepo::get_checkpoints(&pool, &session.id)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, cp2.id); // most recent first
        assert_eq!(all[1].id, cp1.id);

        // get_latest_checkpoint returns cp2
        let latest = SessionRepo::get_latest_checkpoint(&pool, &session.id)
            .await
            .unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, cp2.id);

        // delete_checkpoint removes cp1
        SessionRepo::delete_checkpoint(&pool, &cp1.id)
            .await
            .unwrap();
        let remaining = SessionRepo::get_checkpoints(&pool, &session.id)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, cp2.id);
    }

    #[tokio::test]
    async fn test_session_get_latest_checkpoint_empty() {
        let pool = setup_test_db().await;
        let session = SessionRepo::create(&pool, None).await.unwrap();

        let latest = SessionRepo::get_latest_checkpoint(&pool, &session.id)
            .await
            .unwrap();
        assert!(latest.is_none());
    }

    #[tokio::test]
    async fn test_session_list_with_filters() {
        let pool = setup_test_db().await;

        // Create sessions with different statuses
        let s1 = SessionRepo::create(&pool, None).await.unwrap();
        SessionRepo::update_status(&pool, &s1.id, "completed")
            .await
            .unwrap();

        let s2 = SessionRepo::create(&pool, None).await.unwrap();
        SessionRepo::update_status(&pool, &s2.id, "started")
            .await
            .unwrap();

        let s3 = SessionRepo::create(&pool, None).await.unwrap();
        SessionRepo::update_status(&pool, &s3.id, "completed")
            .await
            .unwrap();

        // Filter by status = "completed"
        let filter = ListSessionsFilter {
            status: Some("completed".to_string()),
            ..Default::default()
        };
        let completed = SessionRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(completed.len(), 2);
        assert!(
            completed
                .iter()
                .all(|s| s.status.as_deref() == Some("completed"))
        );

        // Filter by status = "started"
        let filter = ListSessionsFilter {
            status: Some("started".to_string()),
            ..Default::default()
        };
        let started = SessionRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].id, s2.id);
    }

    #[tokio::test]
    async fn test_session_list_in_range() {
        let pool = setup_test_db().await;

        // Create a session (will have created_at = now)
        let s1 = SessionRepo::create(&pool, None).await.unwrap();

        let now = chrono::Utc::now();
        let one_hour_ago = now - chrono::Duration::hours(1);
        let one_hour_ahead = now + chrono::Duration::hours(1);

        // Range that includes now
        let in_range = SessionRepo::list_in_range(&pool, one_hour_ago, one_hour_ahead, None)
            .await
            .unwrap();
        assert_eq!(in_range.len(), 1);
        assert_eq!(in_range[0].id, s1.id);

        // Range in the past (should be empty)
        let two_hours_ago = now - chrono::Duration::hours(2);
        let out_of_range = SessionRepo::list_in_range(&pool, two_hours_ago, one_hour_ago, None)
            .await
            .unwrap();
        assert!(out_of_range.is_empty());
    }

    #[tokio::test]
    async fn test_session_update_summary() {
        let pool = setup_test_db().await;
        let session = SessionRepo::create(&pool, None).await.unwrap();

        let tools = serde_json::json!(["read", "write", "grep"]);
        let updated = SessionRepo::update_summary(
            &pool,
            &session.id,
            "Implemented feature X",
            Some(0.05),
            Some(1000),
            Some(500),
            Some(120.0),
            Some(&tools),
        )
        .await
        .unwrap();

        assert_eq!(updated.summary.as_deref(), Some("Implemented feature X"));
        assert!((updated.cost.unwrap() - 0.05).abs() < f64::EPSILON);
        assert_eq!(updated.input_tokens, Some(1000));
        assert_eq!(updated.output_tokens, Some(500));
        assert!((updated.duration_seconds.unwrap() - 120.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_session_delete() {
        let pool = setup_test_db().await;
        let session = SessionRepo::create(&pool, None).await.unwrap();

        SessionRepo::delete(&pool, &session.id).await.unwrap();

        let result = SessionRepo::get(&pool, &session.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_delete_cascades_checkpoints() {
        let pool = setup_test_db().await;
        let session = SessionRepo::create(&pool, None).await.unwrap();

        let data = serde_json::json!({"test": true});
        let cp = SessionRepo::save_checkpoint(&pool, &session.id, &data)
            .await
            .unwrap();

        // Deleting session should cascade to checkpoints
        SessionRepo::delete(&pool, &session.id).await.unwrap();

        // Checkpoint should no longer be retrievable
        let cps = SessionRepo::get_checkpoints(&pool, &session.id)
            .await
            .unwrap();
        assert!(cps.is_empty());

        // Deleting the checkpoint directly should also fail
        let result = SessionRepo::delete_checkpoint(&pool, &cp.id).await;
        assert!(result.is_err());
    }
}

// ============================================================
// Reflection tests
// ============================================================

mod reflection {
    use super::*;

    /// Helper: create a session and return its ID (reflections require a session FK).
    async fn create_session(pool: &PgPool) -> String {
        SessionRepo::create(pool, None).await.unwrap().id
    }

    fn make_reflection(session_id: &str) -> CreateReflection {
        CreateReflection {
            session_id: session_id.to_string(),
            what_worked: Some("Tests passed on first run".to_string()),
            what_failed: Some("CI pipeline was slow".to_string()),
            lessons_learned: Some("Cache dependencies for faster builds".to_string()),
            effectiveness_score: Some(0.8),
            complexity_score: Some(0.4),
            kind: Some("session_end".to_string()),
            action_items: None,
            overall_score: Some(0.75),
            knowledge_score: Some(0.8),
            decision_score: Some(0.7),
            efficiency_score: Some(0.6),
            evaluated_episode_ids: None,
            project: None,
        }
    }

    #[tokio::test]
    async fn test_reflection_create_and_get() {
        let pool = setup_test_db().await;
        let session_id = create_session(&pool).await;
        let input = make_reflection(&session_id);

        let created = ReflectionRepo::create(&pool, &input, None).await.unwrap();
        assert!(!created.id.is_empty());
        assert_eq!(created.session_id, session_id);
        assert_eq!(
            created.what_worked.as_deref(),
            Some("Tests passed on first run")
        );
        assert_eq!(created.what_failed.as_deref(), Some("CI pipeline was slow"));
        assert_eq!(
            created.lessons_learned.as_deref(),
            Some("Cache dependencies for faster builds")
        );
        assert!((created.effectiveness_score.unwrap() - 0.8).abs() < f64::EPSILON);
        assert!((created.complexity_score.unwrap() - 0.4).abs() < f64::EPSILON);
        assert_eq!(created.kind, "session_end");
        assert!((created.overall_score.unwrap() - 0.75).abs() < f64::EPSILON);
        assert!((created.knowledge_score.unwrap() - 0.8).abs() < f64::EPSILON);
        assert!((created.decision_score.unwrap() - 0.7).abs() < f64::EPSILON);
        assert!((created.efficiency_score.unwrap() - 0.6).abs() < f64::EPSILON);
        assert!(created.needs_embedding);

        let fetched = ReflectionRepo::get(&pool, &created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.session_id, session_id);
    }

    #[tokio::test]
    async fn test_reflection_list_filters() {
        let pool = setup_test_db().await;
        let s1 = create_session(&pool).await;
        let s2 = create_session(&pool).await;

        // Create session_end reflection
        let mut r1 = make_reflection(&s1);
        r1.kind = Some("session_end".to_string());
        ReflectionRepo::create(&pool, &r1, None).await.unwrap();

        // Create periodic reflection
        let mut r2 = make_reflection(&s2);
        r2.kind = Some("periodic".to_string());
        ReflectionRepo::create(&pool, &r2, None).await.unwrap();

        // Filter by kind
        let filter = ListReflectionsFilter {
            kind: Some("periodic".to_string()),
            ..Default::default()
        };
        let items = ReflectionRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "periodic");

        // Filter by session_id
        let filter = ListReflectionsFilter {
            session_id: Some(s1.clone()),
            ..Default::default()
        };
        let items = ReflectionRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].session_id, s1);
    }

    #[tokio::test]
    async fn test_reflection_list_filter_by_project() {
        let pool = setup_test_db().await;
        let proj = ProjectRepo::get_or_create(&pool, "refl-proj", None)
            .await
            .unwrap();
        let s1 = create_session(&pool).await;
        let s2 = create_session(&pool).await;

        let r1 = make_reflection(&s1);
        ReflectionRepo::create(&pool, &r1, Some(&proj.id))
            .await
            .unwrap();

        let r2 = make_reflection(&s2);
        ReflectionRepo::create(&pool, &r2, None).await.unwrap();

        let filter = ListReflectionsFilter {
            project: Some(proj.id.clone()),
            ..Default::default()
        };
        let items = ReflectionRepo::list(&pool, &filter).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].session_id, s1);
    }

    #[tokio::test]
    async fn test_reflection_fts_search() {
        let pool = setup_test_db().await;
        let session_id = create_session(&pool).await;

        let mut input = make_reflection(&session_id);
        input.lessons_learned = Some(
            "Always use database connection pooling to avoid exhaustion under load".to_string(),
        );
        ReflectionRepo::create(&pool, &input, None).await.unwrap();

        // Another reflection with different content
        let s2 = create_session(&pool).await;
        let mut input2 = make_reflection(&s2);
        input2.lessons_learned =
            Some("Kubernetes pod autoscaling requires proper resource limits".to_string());
        ReflectionRepo::create(&pool, &input2, None).await.unwrap();

        // FTS search for "connection pooling"
        let results = ReflectionRepo::fts_search(&pool, "connection pooling", None, 10)
            .await
            .unwrap();
        assert!(
            !results.is_empty(),
            "FTS should find connection pooling reflection"
        );
        assert!(results[0].1.contains("connection pooling"));
    }

    #[tokio::test]
    async fn test_reflection_fts_search_empty() {
        let pool = setup_test_db().await;
        let session_id = create_session(&pool).await;

        let input = make_reflection(&session_id);
        ReflectionRepo::create(&pool, &input, None).await.unwrap();

        let results = ReflectionRepo::fts_search(&pool, "quantum teleportation", None, 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_reflection_score_trends() {
        let pool = setup_test_db().await;
        let session_id = create_session(&pool).await;

        // Create a reflection with scores (created_at = now, so within 30 days)
        let input = make_reflection(&session_id);
        ReflectionRepo::create(&pool, &input, None).await.unwrap();

        let s2 = create_session(&pool).await;
        let mut input2 = make_reflection(&s2);
        input2.overall_score = Some(0.9);
        input2.knowledge_score = Some(0.95);
        ReflectionRepo::create(&pool, &input2, None).await.unwrap();

        // Both reflections are today, so we should get 1 row with averaged scores
        let trends = ReflectionRepo::score_trends(&pool, None, 30).await.unwrap();
        assert_eq!(trends.len(), 1);
        assert_eq!(trends[0].count, 2);

        // Average of 0.75 and 0.9 = 0.825
        let avg_overall = trends[0].avg_overall.unwrap();
        assert!(
            (avg_overall - 0.825).abs() < 0.001,
            "Expected avg_overall ≈ 0.825, got {avg_overall}"
        );

        // Average of 0.8 and 0.95 = 0.875
        let avg_knowledge = trends[0].avg_knowledge.unwrap();
        assert!(
            (avg_knowledge - 0.875).abs() < 0.001,
            "Expected avg_knowledge ≈ 0.875, got {avg_knowledge}"
        );
    }

    #[tokio::test]
    async fn test_reflection_delete() {
        let pool = setup_test_db().await;
        let session_id = create_session(&pool).await;
        let input = make_reflection(&session_id);
        let created = ReflectionRepo::create(&pool, &input, None).await.unwrap();

        ReflectionRepo::delete(&pool, &created.id).await.unwrap();

        let result = ReflectionRepo::get(&pool, &created.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reflection_find_needing_embedding() {
        let pool = setup_test_db().await;
        let session_id = create_session(&pool).await;
        let input = make_reflection(&session_id);
        let created = ReflectionRepo::create(&pool, &input, None).await.unwrap();
        assert!(created.needs_embedding);

        let needing = ReflectionRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert_eq!(needing.len(), 1);

        ReflectionRepo::mark_embedded(&pool, &created.id)
            .await
            .unwrap();

        let needing = ReflectionRepo::find_needing_embedding(&pool, 10)
            .await
            .unwrap();
        assert!(needing.is_empty());
    }
}

// ============================================================
// Raptor tests
// ============================================================

mod raptor {
    use super::*;

    #[tokio::test]
    async fn test_raptor_tree_lifecycle() {
        let pool = setup_test_db().await;
        let proj = ProjectRepo::get_or_create(&pool, "raptor-proj", None)
            .await
            .unwrap();

        // Upsert tree for project
        let tree = RaptorRepo::upsert_tree(&pool, Some(&proj.id))
            .await
            .unwrap();
        assert!(!tree.id.is_empty());
        assert_eq!(tree.project_id.as_deref(), Some(proj.id.as_str()));
        assert_eq!(tree.status, "building");
        assert_eq!(tree.total_nodes, 0);
        assert_eq!(tree.max_depth, 0);

        // Insert nodes at different levels
        let node0 = RaptorRepo::insert_node(
            &pool,
            &tree.id,
            0,    // level
            None, // no parent (root)
            "knowledge",
            "entity-1",
            Some("Summary of entity-1"),
            2, // children_count
        )
        .await
        .unwrap();
        assert_eq!(node0.tree_id, tree.id);
        assert_eq!(node0.level, 0);
        assert!(node0.parent_id.is_none());
        assert_eq!(node0.entity_type, "knowledge");

        let node1 = RaptorRepo::insert_node(
            &pool,
            &tree.id,
            1,
            Some(&node0.id),
            "raptor_node",
            "cluster-1",
            Some("Cluster summary"),
            0,
        )
        .await
        .unwrap();
        assert_eq!(node1.level, 1);
        assert_eq!(node1.parent_id.as_deref(), Some(node0.id.as_str()));

        // get_collapsed_tree returns nodes ordered by level ASC
        let nodes = RaptorRepo::get_collapsed_tree(&pool, &tree.id)
            .await
            .unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].level, 0);
        assert_eq!(nodes[1].level, 1);

        // Update tree stats
        RaptorRepo::update_tree_stats(&pool, &tree.id, 2, 1, "ready")
            .await
            .unwrap();
        let updated_tree = RaptorRepo::get_tree_by_id(&pool, &tree.id).await.unwrap();
        assert_eq!(updated_tree.total_nodes, 2);
        assert_eq!(updated_tree.max_depth, 1);
        assert_eq!(updated_tree.status, "ready");

        // Delete all nodes
        RaptorRepo::delete_tree_nodes(&pool, &tree.id)
            .await
            .unwrap();
        let nodes = RaptorRepo::get_collapsed_tree(&pool, &tree.id)
            .await
            .unwrap();
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn test_raptor_global_tree() {
        let pool = setup_test_db().await;

        // Upsert global tree (project_id = None) twice
        let tree1 = RaptorRepo::upsert_tree(&pool, None).await.unwrap();
        assert!(tree1.project_id.is_none());

        let tree2 = RaptorRepo::upsert_tree(&pool, None).await.unwrap();
        // Should return the same tree (no duplicates for NULL project_id)
        assert_eq!(tree1.id, tree2.id);
    }

    #[tokio::test]
    async fn test_raptor_project_tree_idempotent() {
        let pool = setup_test_db().await;
        let proj = ProjectRepo::get_or_create(&pool, "raptor-idem", None)
            .await
            .unwrap();

        let tree1 = RaptorRepo::upsert_tree(&pool, Some(&proj.id))
            .await
            .unwrap();
        let tree2 = RaptorRepo::upsert_tree(&pool, Some(&proj.id))
            .await
            .unwrap();

        // ON CONFLICT on project_id → same tree returned
        assert_eq!(tree1.id, tree2.id);
        assert_eq!(tree2.status, "building"); // Reset on upsert
    }

    #[tokio::test]
    async fn test_raptor_get_tree() {
        let pool = setup_test_db().await;

        // No tree yet
        let none = RaptorRepo::get_tree(&pool, None).await.unwrap();
        assert!(none.is_none());

        // Create one
        RaptorRepo::upsert_tree(&pool, None).await.unwrap();

        let found = RaptorRepo::get_tree(&pool, None).await.unwrap();
        assert!(found.is_some());
    }
}

// ============================================================
// SearchQuery tests
// ============================================================

mod search_query {
    use super::*;

    #[tokio::test]
    async fn test_search_query_log_and_click() {
        let pool = setup_test_db().await;

        let result_ids = vec![
            "item-a".to_string(),
            "item-b".to_string(),
            "item-c".to_string(),
        ];
        let query_id = SearchQueryRepo::log(
            &pool,
            "rust error handling",
            None,
            &result_ids,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!query_id.is_empty());

        // Record a click on item-b
        SearchQueryRepo::record_click(&pool, "item-b")
            .await
            .unwrap();

        // Verify: retrieve the search query row and check clicked_ids
        let row = sqlx::query_as::<_, (Vec<String>,)>(
            "SELECT clicked_ids FROM search_queries WHERE id = $1",
        )
        .bind(&query_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row.0.contains(&"item-b".to_string()));
    }

    #[tokio::test]
    async fn test_search_query_click_not_in_results_is_noop() {
        let pool = setup_test_db().await;

        let result_ids = vec!["item-a".to_string()];
        let query_id =
            SearchQueryRepo::log(&pool, "test query", None, &result_ids, None, None, None)
                .await
                .unwrap();

        // Click on an item NOT in result_ids — should be a no-op
        SearchQueryRepo::record_click(&pool, "item-z")
            .await
            .unwrap();

        let row = sqlx::query_as::<_, (Vec<String>,)>(
            "SELECT clicked_ids FROM search_queries WHERE id = $1",
        )
        .bind(&query_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row.0.is_empty());
    }

    #[tokio::test]
    async fn test_feedback_aggregation() {
        let pool = setup_test_db().await;

        // Create a knowledge item so aggregate_feedback has a target row
        let k_input = CreateKnowledge {
            title: "Feedback Target".to_string(),
            content: "test content".to_string(),
            description: None,
            kind: Some("artifact".to_string()),
            language: None,
            file_path: None,
            project: None,
            tags: None,
            valid_from: None,
            valid_until: None,
            source: None,
            source_metadata: None,
        };
        let knowledge = KnowledgeRepo::create(&pool, &k_input, None).await.unwrap();

        // Log a search query where this item appeared in results
        let result_ids = vec![knowledge.id.clone()];
        SearchQueryRepo::log(&pool, "feedback test", None, &result_ids, None, None, None)
            .await
            .unwrap();

        // Click on it
        SearchQueryRepo::record_click(&pool, &knowledge.id)
            .await
            .unwrap();

        // Aggregate feedback — should update feedback_boost on the knowledge item
        let total = SearchQueryRepo::aggregate_feedback(&pool).await.unwrap();
        assert!(
            total >= 1,
            "Should have updated at least 1 row, got {total}"
        );

        // Verify feedback_boost was set on the knowledge item
        let row = sqlx::query_as::<_, (Option<f64>,)>(
            "SELECT feedback_boost FROM knowledge_items WHERE id = $1",
        )
        .bind(&knowledge.id)
        .fetch_one(&pool)
        .await
        .unwrap();

        // CTR = 1 click / 1 shown = 1.0, capped at 1.0
        let boost = row.0.unwrap_or(0.0);
        assert!(
            boost > 0.0,
            "feedback_boost should be > 0 after a click, got {boost}"
        );
    }
}

// ============================================================
// Vault tests
// ============================================================

mod vault {
    use super::*;

    /// Helper: create an owner and return their ID.
    async fn create_owner(pool: &PgPool, username: &str) -> String {
        OwnerRepo::create(pool, username, "hashed-password-placeholder")
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn test_vault_store_and_get() {
        let pool = setup_test_db().await;
        let owner_id = create_owner(&pool, "vault-user").await;

        let encrypted = b"encrypted-api-key-data";
        let nonce = b"random-nonce-12b";

        let secret = VaultRepo::store(
            &pool,
            &owner_id,
            "openai_key",
            encrypted,
            nonce,
            Some("OpenAI API key"),
        )
        .await
        .unwrap();

        assert!(!secret.id.is_empty());
        assert_eq!(secret.owner_id, owner_id);
        assert_eq!(secret.name, "openai_key");
        assert_eq!(secret.encrypted_value, encrypted.to_vec());
        assert_eq!(secret.nonce, nonce.to_vec());
        assert_eq!(secret.description.as_deref(), Some("OpenAI API key"));

        // Retrieve by name
        let fetched = VaultRepo::get_by_name(&pool, &owner_id, "openai_key")
            .await
            .unwrap();
        assert_eq!(fetched.id, secret.id);
        assert_eq!(fetched.encrypted_value, encrypted.to_vec());
    }

    #[tokio::test]
    async fn test_vault_upsert() {
        let pool = setup_test_db().await;
        let owner_id = create_owner(&pool, "vault-upsert-user").await;

        let encrypted1 = b"old-encrypted-value";
        let nonce1 = b"old-nonce-12byte";

        VaultRepo::store(
            &pool,
            &owner_id,
            "my_secret",
            encrypted1,
            nonce1,
            Some("v1"),
        )
        .await
        .unwrap();

        // Store again with same name → should upsert (update)
        let encrypted2 = b"new-encrypted-value";
        let nonce2 = b"new-nonce-12byte";

        VaultRepo::store(
            &pool,
            &owner_id,
            "my_secret",
            encrypted2,
            nonce2,
            Some("v2"),
        )
        .await
        .unwrap();

        let fetched = VaultRepo::get_by_name(&pool, &owner_id, "my_secret")
            .await
            .unwrap();
        assert_eq!(fetched.encrypted_value, encrypted2.to_vec());
        assert_eq!(fetched.nonce, nonce2.to_vec());
        assert_eq!(fetched.description.as_deref(), Some("v2"));

        // Only one secret with that name should exist
        let all = VaultRepo::list(&pool, &owner_id).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn test_vault_delete() {
        let pool = setup_test_db().await;
        let owner_id = create_owner(&pool, "vault-delete-user").await;

        VaultRepo::store(&pool, &owner_id, "to_delete", b"val", b"nonce12bytes", None)
            .await
            .unwrap();

        VaultRepo::delete(&pool, &owner_id, "to_delete")
            .await
            .unwrap();

        let result = VaultRepo::get_by_name(&pool, &owner_id, "to_delete").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vault_delete_nonexistent() {
        let pool = setup_test_db().await;
        let owner_id = create_owner(&pool, "vault-del-noexist").await;

        let result = VaultRepo::delete(&pool, &owner_id, "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vault_get_nonexistent() {
        let pool = setup_test_db().await;
        let owner_id = create_owner(&pool, "vault-get-noexist").await;

        let result = VaultRepo::get_by_name(&pool, &owner_id, "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vault_list() {
        let pool = setup_test_db().await;
        let owner_id = create_owner(&pool, "vault-list-user").await;

        VaultRepo::store(&pool, &owner_id, "alpha_key", b"v1", b"n1-12bytes!!", None)
            .await
            .unwrap();
        VaultRepo::store(&pool, &owner_id, "beta_key", b"v2", b"n2-12bytes!!", None)
            .await
            .unwrap();

        let secrets = VaultRepo::list(&pool, &owner_id).await.unwrap();
        assert_eq!(secrets.len(), 2);
        // Ordered by name ASC
        assert_eq!(secrets[0].name, "alpha_key");
        assert_eq!(secrets[1].name, "beta_key");
    }

    #[tokio::test]
    async fn test_vault_isolation_between_owners() {
        let pool = setup_test_db().await;
        let owner1 = create_owner(&pool, "owner-one").await;
        let owner2 = create_owner(&pool, "owner-two").await;

        VaultRepo::store(
            &pool,
            &owner1,
            "shared_name",
            b"val1",
            b"nonce1-12byt",
            None,
        )
        .await
        .unwrap();
        VaultRepo::store(
            &pool,
            &owner2,
            "shared_name",
            b"val2",
            b"nonce2-12byt",
            None,
        )
        .await
        .unwrap();

        let s1 = VaultRepo::get_by_name(&pool, &owner1, "shared_name")
            .await
            .unwrap();
        let s2 = VaultRepo::get_by_name(&pool, &owner2, "shared_name")
            .await
            .unwrap();

        assert_ne!(s1.id, s2.id);
        assert_eq!(s1.encrypted_value, b"val1");
        assert_eq!(s2.encrypted_value, b"val2");
    }
}

// ============================================================
// Graph edge additional tests
// ============================================================

mod graph_extended {
    use super::*;

    #[tokio::test]
    async fn test_graph_edge_unique_constraint_updates() {
        let pool = setup_test_db().await;

        let input = CreateRelation {
            source_type: "knowledge".to_string(),
            source_id: "src-1".to_string(),
            target_type: "knowledge".to_string(),
            target_id: "tgt-1".to_string(),
            relation: "depends_on".to_string(),
            weight: Some(0.5),
            description: Some("first insert".to_string()),
            metadata: None,
        };

        let first = GraphRepo::create_edge(&pool, &input).await.unwrap();
        assert_eq!(first.usage_count, 1);
        assert!((first.weight - 0.5).abs() < f64::EPSILON);

        // Insert the same edge again — ON CONFLICT should increment usage_count
        let second = GraphRepo::create_edge(&pool, &input).await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.usage_count, 2);
    }

    #[tokio::test]
    async fn test_graph_decay_then_verify_weight() {
        let pool = setup_test_db().await;

        let input = CreateRelation {
            source_type: "knowledge".to_string(),
            source_id: "decay-src".to_string(),
            target_type: "knowledge".to_string(),
            target_id: "decay-tgt".to_string(),
            relation: "relates_to".to_string(),
            weight: Some(1.0),
            description: None,
            metadata: None,
        };
        let edge = GraphRepo::create_edge(&pool, &input).await.unwrap();

        // Apply decay twice: 1.0 * 0.95 = 0.95, then 0.95 * 0.95 = 0.9025
        GraphRepo::decay_weights(&pool).await.unwrap();
        GraphRepo::decay_weights(&pool).await.unwrap();

        let decayed = GraphRepo::get_edge(&pool, &edge.id).await.unwrap();
        assert!(
            (decayed.weight - 0.9025).abs() < 0.001,
            "Expected ≈0.9025 after two decays, got {}",
            decayed.weight
        );
    }
}
