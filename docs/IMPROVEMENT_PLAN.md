# Alaz İyileştirme Planı

**Tarih**: 2026-03-31
**Mevcut durum**: 7.5/10 — 18.8K satır, 613 fonksiyon, 105 test, 9 crate
**Hedef**: 9/10 — %60+ test coverage, sıfır duplication, tutarlı error handling, migration tracking

## Genel Strateji

5 faz, her biri bağımsız olarak deploy edilebilir. Her faz öncekinin üzerine inşa eder ama hard dependency yok — acil ihtiyaca göre sıra değişebilir.

---

## Faz 1: Unified Error Handling (1 gün)

**Neden önce**: Diğer tüm fazlar (test, refactor) tutarlı error handling'e bağlı. Mevcut durum: bazı handler'lar `e.to_string()`, bazıları `format!("serialization error: {e}")`, bazıları hata yutuyoir.

### 1.1 — API Error Response Type
```
crates/alaz-server/src/error.rs (YENİ)
```
- `ApiError` enum oluştur: `NotFound`, `BadRequest`, `Internal`, `Auth`, `Validation`
- `IntoResponse` implemente et → her variant tutarlı JSON dönsün:
  ```json
  { "error": { "code": "not_found", "message": "episode xyz not found" } }
  ```
- `AlazError` → `ApiError` otomatik dönüşüm (`From<AlazError>`)
- HTTP status code mapping: `NotFound→404`, `Validation→400`, `Auth→401`, `*→500`

### 1.2 — Tüm REST Handler'ları Migrate Et
```
crates/alaz-server/src/api/*.rs (13 dosya)
```
- Her handler'daki `match ... Ok/Err` pattern'ini `?` operatörüne çevir
- `impl IntoResponse` return type'ını `Result<Json<T>, ApiError>`'a çevir
- **Öncesi**: ~50 satır boilerplate per handler
- **Sonrası**: ~15 satır per handler

### 1.3 — MCP Error Standardization
```
crates/alaz-server/src/mcp/mod.rs
```
- MCP tool fonksiyonlarındaki `format!("... failed: {e}")` pattern'lerini sınıflandır
- User-facing error message'ları temizle (internal detay leak'lerini kaldır)

**Çıktı**: Tek bir error type, tüm API'lerde tutarlı JSON error response'ları.

---

## Faz 2: Code Duplication Temizliği (1-2 gün)

### 2.1 — Generic Dedup Trait (`alaz-intel`)
```
crates/alaz-intel/src/dedup.rs (YENİ)
```
Şu anda `learner.rs`'de 3 neredeyse identik fonksiyon var:
- `is_duplicate_knowledge()` — 30 satır
- `is_duplicate_episode()` — 30 satır  
- `is_duplicate_procedure()` — 30 satır

**Plan**:
```rust
trait DedupTarget {
    fn entity_type_name(&self) -> &'static str;
    async fn find_similar_by_title(pool: &PgPool, title: &str, threshold: f32, project: Option<&str>) -> Result<bool>;
}

async fn is_duplicate<T: DedupTarget>(
    pool: &PgPool, qdrant: &Qdrant, embedding: &EmbeddingService,
    session_buf: &SessionDedup, title: &str, project: Option<&str>,
    trigram_threshold: f32, vector_threshold: f32,
) -> Result<bool>
```

3×30 = 90 satır → 1×40 satır generic + 3×5 satır impl = 55 satır. ~%40 azalma.

### 2.2 — Feedback Aggregation SQL Deduplication
```
crates/alaz-db/src/repos/search_query.rs
```
3 kere tekrar eden CTR aggregation query'sini tek bir parametrik fonksiyona çevir:
```rust
async fn update_feedback_for_table(pool: &PgPool, table: &str) -> Result<u64>
```
**Not**: `table` parametresi compile-time constant olarak garanti edilecek (enum veya const array).

~90 satır SQL → ~35 satır.

### 2.3 — `jobs.rs` Macro → Generic Function
```
crates/alaz-server/src/jobs.rs
```
`embed_entity_batch!` macro'sunu async generic fonksiyona çevir. Macro'nun problemi:
- Debug edilemez (stack trace'ler macro expansion'a işaret eder)
- IDE desteği zayıf
- Rust 2024 edition'da async closure'lar stabil

**Yaklaşım**: `mark_embedded` callback'ini generic fonksiyon parametre olarak al:
```rust
async fn embed_batch<E: Embeddable>(
    items: Vec<E>,
    pool: &PgPool, qdrant: &QdrantManager,
    embedding: &EmbeddingService, colbert: &ColbertService,
    mark_embedded: impl Fn(&PgPool, &str) -> Pin<Box<dyn Future<Output=Result<()>> + Send + '_>>,
    label: &str,
) -> (u32, u32)
```

### 2.4 — FTS Signal → UNION ALL
```
crates/alaz-search/src/signals/fts.rs
```
3 sequential query → tek `UNION ALL` query (proactive.rs'deki pattern gibi).

**Çıktı**: ~200 satır net azalma, bakımı kolay kod.

---

## Faz 3: Test Coverage (3-5 gün) — EN BÜYÜK FAZ

**Mevcut**: 105 test, ~%5 coverage
**Hedef**: 300+ test, ~%60 coverage

### Strateji
Unit test'ler pure fonksiyonlara, integration test'ler DB gerektiren kodlara. Crate sırasına göre bottom-up.

### 3.1 — `alaz-core` Test Genişletme
```
crates/alaz-core/src/ — mevcut: 21 test
```
- `error.rs`: Error dönüşüm test'leri (From<sqlx::Error>, Display format)
- `models/*.rs`: Serialization/deserialization round-trip testleri (her model için)
- `traits.rs`: `Embeddable` trait default implementations
- **Hedef**: +15 test → 36

### 3.2 — `alaz-auth` Test Genişletme
```
crates/alaz-auth/src/ — mevcut: 11 test
```
- `middleware.rs`: Mock request ile `AuthUser` extraction (Bearer, API key, missing auth)
- `jwt.rs`: Expired token, empty secret, malformed token edge cases
- **Hedef**: +8 test → 19

### 3.3 — `alaz-vector` Test'leri (SIFIRDAN)
```
crates/alaz-vector/tests/unit.rs (YENİ)
```
- `dense.rs`: `point_id()` determinism, aynı input → aynı UUID
- `colbert.rs`: `average_embedding()`, `cosine_similarity()`, `max_sim()` — pure math fonksiyonları
- `client.rs`: `COLLECTION_TEXT`, `COLLECTION_COLBERT` constant değerleri
- **Hedef**: +12 test

### 3.4 — `alaz-db` Integration Test Genişletme
```
crates/alaz-db/tests/integration.rs — mevcut test sayısı kontrol et
```
Her repo için CRUD cycle testi:
- `KnowledgeRepo`: create → get → update → fts_search → find_similar → supersede → delete
- `EpisodeRepo`: create → list (filter by type, resolved) → cue_search → find_by_files → resolve → delete
- `ProcedureRepo`: create → record_outcome (success/failure) → Wilson score doğrulama → delete
- `CoreMemoryRepo`: upsert (create) → upsert (update, same key) → find_similar_by_key → delete
- `SessionRepo`: create → ensure_exists → save_checkpoint → get_latest_checkpoint → delete
- `ReflectionRepo`: create → list (filter by kind) → fts_search → score_trends → delete
- `RaptorRepo`: upsert_tree (project) → upsert_tree (NULL/global) → insert_node → get_collapsed_tree → delete_tree_nodes
- `SearchQueryRepo`: log → record_click → aggregate_feedback doğrulama
- `GraphRepo`: create_edge → get_edges (outgoing/incoming) → increment_usage → decay_weights
- **Hedef**: +40 test

### 3.5 — `alaz-intel` Test Genişletme
```
crates/alaz-intel/src/ — mevcut: 22 test
```
Pure logic testleri (LLM/DB gerektirmeyen):
- `learner.rs`: `chunk_transcript()` — büyük transcript, UTF-8 sınır, [USER]: marker split
- `context.rs`: `format_section()`, `truncate()` — edge cases (empty, UTF-8 sınır, tam sınır)
- `compact.rs`: `truncate_str()` — aynı edge cases
- `optimizer.rs`: `cleanup_whitespace()`, `split_sections()` — daha fazla edge case
- `contradiction.rs`: Test skeleton'ları (mock LLM ile veya yalnız struct/enum testleri)
- `hyde.rs`: HyDE prompt generation (LLM çağrısı olmadan prompt oluşturma testi)
- `embeddings.rs`: Request serialization, error handling
- **Hedef**: +20 test → 42

### 3.6 — `alaz-graph` Test Genişletme
```
crates/alaz-graph/src/ — mevcut: 4 test
```
- `scoring.rs`: Boundary değerler (0 usage, MAX age, negative elapsed)
- `causal.rs`: Causal relation filtering (sadece `CAUSAL_RELATIONS` listesindekiler)
- `traversal.rs`: BFS max_depth cap (10), empty graph, cycle detection (visited set)
- **Hedef**: +8 test → 12

### 3.7 — `alaz-search` Test Genişletme
```
crates/alaz-search/src/ — mevcut: 48 test
```
- `rerank.rs`: Cache hit/miss, score normalization (tüm aynı skor, tek skor, negatif)
- `proactive.rs`: `extract_keywords()` — daha fazla tool type, edge cases
- `pipeline.rs`: Query classifier integration (query type → doğru weight'ler)
- **Hedef**: +15 test → 63

### 3.8 — `alaz-server` Test'leri (SIFIRDAN)
```
crates/alaz-server/tests/api.rs (YENİ)
```
`axum::test` veya `tower::ServiceExt` ile HTTP-level test:
- `rate_limit.rs`: `RateLimiter` unit test'leri — check, cleanup, token refill
- `router.rs`: Health endpoint response format, proactive handler (mock pool)
- API endpoint smoke tests (create knowledge → search → get → delete)
- **Hedef**: +20 test

### 3.9 — `alaz-cli` Test'leri (SIFIRDAN)
```
crates/alaz-cli/src/main.rs → tests modülü
```
- `read_transcript_file()`: JSONL parsing, plain text fallback, empty file
- `has_alaz_marker()`: marker var/yok, nested directory, root boundary
- `project_name_from_cwd()`: normal path, root, trailing slash
- `read_hook_input()`: valid JSON, invalid JSON, empty stdin
- **Hedef**: +12 test

**Faz 3 Toplam**: ~150 yeni test → 255 toplam, ~%60 function coverage.

---

## Faz 4: Migration System (0.5 gün)

### 4.1 — Migration Tracking Table
```
crates/alaz-db/src/migrations/000_migration_tracking.sql (YENİ)
```
```sql
CREATE TABLE IF NOT EXISTS _alaz_migrations (
    version TEXT PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 4.2 — Smart Migration Runner
```
crates/alaz-db/src/lib.rs — run_migrations() refactor
```
Mevcut: tüm SQL dosyalarını her startup'ta `raw_sql` ile çalıştır.
Yeni:
```rust
pub async fn run_migrations(pool: &PgPool) -> Result<u32> {
    // 1. Ensure _alaz_migrations table exists
    // 2. SELECT version FROM _alaz_migrations
    // 3. For each migration file NOT in applied list:
    //    a. BEGIN transaction
    //    b. Execute SQL
    //    c. INSERT INTO _alaz_migrations (version)
    //    d. COMMIT
    // 4. Return count of newly applied migrations
}
```
- Migration dosyaları sıralı kalır (001_, 002_, ...)
- Her migration transactional — fail ederse rollback
- Bir kere çalışan migration tekrar çalışmaz
- `DROP COLUMN` gibi non-idempotent migration'lar artık güvenli

### 4.3 — Migration CLI Enhancement
```
crates/alaz-cli/src/main.rs
```
- `alaz migrate` → mevcut davranış (unapplied migration'ları çalıştır)
- `alaz migrate --status` → hangi migration'lar uygulanmış/bekliyor göster
- `alaz migrate --dry-run` → hangileri çalışacak göster, çalıştırma

**Çıktı**: Her startup'ta tüm SQL'lerin blind replay'i yerine proper migration tracking.

---

## Faz 5: Kalan Tech Debt & Docs (1-2 gün)

### 5.1 — N+1 Query Fix (`context.rs`)
```
crates/alaz-intel/src/context.rs
```
FTS seed → `KnowledgeRepo::get()` loop pattern'ini `get_many()` ile değiştir:
```rust
// Önce: N+1
for id in pattern_ids.iter().take(10) {
    if let Ok(item) = KnowledgeRepo::get(&self.pool, id).await { ... }
}

// Sonra: 1 query
let items = KnowledgeRepo::get_many(&self.pool, &pattern_ids[..10]).await?;
```
`KnowledgeRepo::get_many()` fonksiyonunu ekle:
```sql
SELECT ... FROM knowledge_items WHERE id = ANY($1)
```

### 5.2 — `jobs.rs` SQL Injection Hardening
```
crates/alaz-server/src/jobs.rs
```
`format!()` ile SQL string oluşturma pattern'ini kaldır. Enum-based yaklaşım:
```rust
enum DecayableTable {
    KnowledgeItems,
    Episodes,
    Procedures,
}

impl DecayableTable {
    fn table_name(&self) -> &'static str { ... }
    fn entity_type(&self) -> &'static str { ... }
    
    async fn decay(&self, pool: &PgPool) -> u64 {
        // Her variant için ayrı sqlx::query! (compile-time checked)
    }
}
```

### 5.3 — Dead Code Temizliği
- `CueSearchBody.limit` — ya kullan ya kaldır (#[allow(dead_code)] kaldır)
- `port_from_url` — zaten kaldırdık ✅
- `RerankResponseItem.index` — `#[allow(dead_code)]` gerekli mi kontrol et

### 5.4 — Architecture Documentation
```
docs/ARCHITECTURE.md (YENİ)
```
- Crate dependency graph (ASCII art)
- Data flow diagramı: request → auth → handler → search pipeline → response
- Background jobs lifecycle
- Search pipeline 6-signal flow
- Deployment topology (Postgres, Qdrant, Ollama, TEI, ColBERT sidecar)

### 5.5 — API Reference
```
docs/API.md (YENİ)
```
- REST endpoints tablosu (method, path, auth, request/response schema)
- MCP tools tablosu (name, description, parameters)
- Error codes tablosu

### 5.6 — Runbook
```
docs/RUNBOOK.md (YENİ)
```
- Migration prosedürü
- Backup/restore
- RAPTOR rebuild
- Embedding model değişimi prosedürü
- Troubleshooting (service down, high latency, disk full)

---

## Özet Tablo

| Faz | Süre | Etki | Risk |
|-----|------|------|------|
| 1. Error Handling | 1 gün | Tüm API tutarlılığı, daha iyi hata mesajları | Düşük — sadece response format değişiyor |
| 2. Duplication | 1-2 gün | ~200 satır azalma, bakım kolaylığı | Düşük — davranış değişmiyor |
| 3. Test Coverage | 3-5 gün | %5 → %60, refactor güvenliği | Orta — DB integration testleri infra gerektirir |
| 4. Migration System | 0.5 gün | Güvenli migration, deploy güvenliği | Düşük — geriye uyumlu |
| 5. Tech Debt & Docs | 1-2 gün | N+1 fix, SQL hardening, docs | Düşük |

**Toplam**: 6.5-10.5 gün

## Uygulama Sırası

```
Faz 1 (Error Handling) ──→ Faz 2 (Duplication) ──→ Faz 3 (Tests)
                                                         ↓
                           Faz 4 (Migration) ←───── Faz 3 bitmeden başlayabilir
                                                         ↓
                                                   Faz 5 (Debt + Docs)
```

Faz 1 ve 2 önce çünkü: temiz error handling ve deduplicated kod = test yazması çok daha kolay.
Faz 4 bağımsız, Faz 3 ile paralel gidebilir.
Faz 5 en son çünkü: önceki fazlarda kod değişecek, docs son hali yansıtmalı.
