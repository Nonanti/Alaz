# Alaz Roadmap — Kişisel AI Bilgi Sistemini Maksimuma Çıkarma

**Tarih**: 2026-03-31
**Mevcut durum**: 19.8K satır, 9 crate, 271 test, 6-signal hybrid search, auto-learning pipeline
**Hedef**: Proaktif, kendini geliştiren, meta-kognitif bir bilgi sistemi

---

## Genel Vizyon

```
Şu an:   Reaktif Bilgi Deposu
          "Soru sor → cevap al"

Hedef:    Proaktif Bilgi Ortağı
          "Ne biliyorum, ne bilmiyorum, ne öğrenmeliyim?"
```

3 ana eksen, 7 faz:

| Eksen | Fazlar | Tema |
|-------|--------|------|
| 🧠 Self-Improving | Faz 1, 2, 3 | Sistem kendini ölçer ve iyileştirir |
| 🔌 Deep Integration | Faz 4, 5 | Veri kaynakları session ötesine geçer |
| 🪞 Meta-Cognition | Faz 6, 7 | Sistem ne bildiğini ve ne bilmediğini bilir |

---

## Faz 1: Adaptive Signal Weights (3-4 gün)

**Neden önce**: En düşük eforla en büyük arama kalitesi artışı. Altyapı var (`search_queries` + CTR).

### Motivasyon

Şu an `SearchWeights` hardcoded:
```rust
QueryType::Semantic => SearchWeights { fts: 1.0, dense: 1.5, raptor: 1.0, graph: 0.5 }
```
Ama hangi signal gerçekten değerli? Bunu ölçmüyoruz. CTR verimiz var ama weight'lere yansımıyor.

### 1.1 — Signal Attribution Tracking

`search_queries` tablosuna signal kaynağı bilgisi ekle:

```sql
-- Migration 012_signal_attribution.sql
ALTER TABLE search_queries ADD COLUMN IF NOT EXISTS signal_scores JSONB DEFAULT '{}';
-- Örnek: {"fts": ["id1", "id2"], "dense": ["id1", "id3"], "graph": ["id4"]}
```

Search pipeline'da her sonucun hangi signal'lardan geldiğini kaydet.

**Dosyalar**: `pipeline.rs`, `search_query.rs`, migration

### 1.2 — Weight Learning Job

Yeni background job (haftalık):

```rust
/// Haftalık signal weight öğrenme job'u.
///
/// Son 7 günlük search_queries verisinden:
/// 1. Her signal için CTR hesapla (tıklanan sonuçların kaç tanesi bu signal'dan geldi?)
/// 2. Signal başarı oranını normalize et
/// 3. Yeni weight'leri hesapla (exponential moving average ile smooth geçiş)
/// 4. `signal_weights` tablosuna kaydet
pub async fn weight_learning_job(pool: PgPool)
```

**Smoothing**: Ani değişimleri önlemek için EMA (α=0.3):
```
new_weight = α * learned_weight + (1-α) * current_weight
```

**Dosyalar**: `jobs.rs`, yeni `signal_weights` tablosu, `classifier.rs`'de dynamic weight loading

### 1.3 — Classifier'da Dynamic Weight Kullanımı

```rust
impl QueryType {
    /// Önce DB'den öğrenilmiş weight'leri dene, yoksa default'a düş.
    pub async fn weights(&self, pool: &PgPool) -> SearchWeights {
        if let Ok(Some(learned)) = SignalWeightRepo::get_latest(pool, self).await {
            learned
        } else {
            self.default_weights()
        }
    }
}
```

**Çıktı**: Arama kalitesi zamanla kendi kendine iyileşen bir sistem.

---

## Faz 2: Knowledge Consolidation (3-4 gün)

**Neden**: 100+ session sonra benzer bilgiler birikerek noise yaratır. Kalite zamanla düşer.

### 2.1 — Consolidation Job (Haftalık)

```rust
/// Bilgi yoğunlaştırma pipeline:
///
/// 1. Tüm knowledge_items'ı project bazında grupla
/// 2. Her grup içinde vector similarity ile cluster'la (threshold: 0.8)
/// 3. 3+ item'lık cluster'lar için LLM ile birleştirme öner
/// 4. Birleştirilmiş item'ı kaydet, eskileri supersede et
/// 5. Graph edge'leri yeni item'a taşı
pub struct ConsolidationPipeline { ... }
```

### 2.2 — Merge Strategy

```
Cluster: [pattern_A, pattern_B, pattern_C] (benzer konu)
         ↓ LLM merge
Yeni:    [pattern_merged] (kapsamlı, tek item)
         ↓
Eski:    pattern_A.superseded_by = pattern_merged
         pattern_B.superseded_by = pattern_merged
         pattern_C.superseded_by = pattern_merged
```

LLM prompt:
```
Bu 3 bilgi parçasını tek, kapsamlı bir bilgiye birleştir.
Çelişkileri belirt. Tekrarları kaldır. En güncel bilgiyi ön planda tut.
```

### 2.3 — Consolidation Raporu

Her çalışmadan sonra:
```
📦 Konsolidasyon Raporu:
- 47 cluster tespit edildi (3+ benzer item)
- 12 cluster birleştirildi → 12 yeni item, 38 item supersede edildi
- 35 cluster atlandı (yeterince benzer değil veya zaten güncel)
- Toplam bilgi: 340 → 314 item (%8 azalma, sıfır bilgi kaybı)
```

**Dosyalar**: `crates/alaz-intel/src/consolidation.rs` (yeni), `jobs.rs`'e ekleme

---

## Faz 3: Search Explainability — `alaz_explain` (2 gün)

**Neden**: Arama kalitesini debug etmeden iyileştiremezsin.

### 3.1 — Explain Mode

```rust
/// Her search result'a neden döndürüldüğünü açıklayan metadata ekle.
#[derive(Debug, Serialize)]
pub struct SearchExplanation {
    /// Hangi signal'lar bu sonucu döndürdü ve her birinin katkısı
    pub signal_contributions: Vec<SignalContribution>,
    /// Decay etkisi (ne kadar düştü/yükseldi)
    pub decay_impact: f64,
    /// Feedback boost etkisi
    pub feedback_boost: f64,
    /// Reranking öncesi vs sonrası sıra
    pub rank_before_rerank: usize,
    pub rank_after_rerank: usize,
}

pub struct SignalContribution {
    pub signal: String,     // "fts", "dense", "colbert", "graph", "raptor", "cue"
    pub score: f64,         // RRF katkısı
    pub rank_in_signal: usize, // Bu signal'daki sırası
}
```

### 3.2 — MCP Tool

```
alaz_explain(query: "deployment pattern", result_id: "abc123")
→
📊 Sonuç Açıklaması: "Alaz Deploy Pipeline Pattern"
  Signal katkıları:
    FTS:     0.016 (rank 1) ████████████ ← en güçlü sinyal
    Dense:   0.012 (rank 3) █████████
    ColBERT: 0.011 (rank 4) ████████
    Graph:   0.005 (rank 8) ███
    RAPTOR:  0.000 (yok)
    Cue:     0.000 (yok)
  Decay: -2% (3 gün önce erişilmiş)
  Feedback: +5% (CTR: 0.5)
  Rerank: 2 → 1 (cross-encoder yükseltti)
```

**Dosyalar**: `pipeline.rs`, MCP tool ekleme

---

## Faz 4: Git Integration (4-5 gün)

**Neden**: Veri kaynağını 2x'e çıkarır. Passive learning'i session dışına taşır.

### 4.1 — Git Hook / Watcher

```bash
# .git/hooks/post-commit
curl -s -X POST http://your-server:3456/api/v1/ingest/git \
  -H "X-API-Key: $ALAZ_API_KEY" \
  -d "{\"repo\": \"$(basename $(pwd))\", \"commit\": \"$(git rev-parse HEAD)\"}"
```

### 4.2 — Commit Analysis Endpoint

```rust
/// POST /api/v1/ingest/git
///
/// 1. `git show <commit>` ile diff ve message al
/// 2. Diff boyutuna göre:
///    - Küçük (< 50 satır): Doğrudan knowledge olarak kaydet
///    - Orta (50-500 satır): LLM ile özet çıkar
///    - Büyük (500+ satır): Dosya bazında chunk'la ve analiz et
/// 3. Değişen dosyaları episode olarak kaydet (what_cues, where_cues)
/// 4. Commit pattern'lerini analiz et (hot files, frequent changes)
pub async fn ingest_git_commit(...)
```

### 4.3 — Hot File Detection

```sql
-- Migration: git_activity tablosu
CREATE TABLE IF NOT EXISTS git_activity (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    commit_hash TEXT NOT NULL,
    file_path TEXT NOT NULL,
    change_type TEXT NOT NULL, -- 'add', 'modify', 'delete'
    lines_added INT DEFAULT 0,
    lines_removed INT DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Haftalık analiz:
```
🔥 Hot Files (son 7 gün):
  1. crates/alaz-intel/src/learner.rs — 8 commit, 120 satır değişiklik
  2. crates/alaz-server/src/mcp/mod.rs — 6 commit, 95 satır
  → Öneri: learner.rs için refactor prosedürü oluştur
```

### 4.4 — Cross-Commit Pattern Detection

```
🔗 Eş Zamanlı Değişim Pattern'leri:
  - learner.rs + context.rs: %80 birlikte değişiyor → coupling yüksek
  - jobs.rs + search_query.rs: %60 birlikte → ortak refactor fırsatı
```

**Dosyalar**: Yeni `crates/alaz-server/src/api/git.rs`, `crates/alaz-db/src/repos/git_activity.rs`, migration

---

## Faz 5: Codebase Awareness (5-7 gün)

**Neden**: Text-level bilgiden AST-level bilgiye geçiş. Kod hakkında "anlayarak" bilgi tutma.

### 5.1 — Rust AST Indexer

```rust
/// syn crate ile Rust dosyalarını parse et:
/// - Fonksiyon imzaları (pub/priv, async, generic params)
/// - Struct/enum tanımları
/// - Trait impl'leri
/// - Module ağacı
pub struct RustIndexer { ... }
```

`tree-sitter` veya `syn` ile:
```
Index: alaz-core/src/stats.rs
  fn wilson_score_lower(successes: i64, total: i64) -> Option<f64>
    deps: [None]
    callers: [pipeline.rs:245, jobs.rs:180]
    complexity: Low
    test_coverage: 12 tests
```

### 5.2 — Dependency Graph

```rust
/// Crate-level ve fonksiyon-level dependency graph.
///
/// Graph edge'lere "depends_on", "implements", "calls" relation'ları ekle.
/// Bu graph, mevcut knowledge graph ile birleşir.
pub async fn build_code_graph(project_path: &str, pool: &PgPool)
```

### 5.3 — Impact Analysis

```
alaz_impact("KnowledgeRepo::find_similar_by_title imzası değişirse ne etkilenir?")
→
🎯 Impact Analysis:
  Doğrudan çağıranlar (3):
    - learner.rs:1124 — is_duplicate_knowledge()
    - contradiction.rs:68 — check()
    - promotion.rs:23 — check_and_promote()
  Dolaylı etki (2):
    - SessionLearner::learn_from_session (learner.rs üzerinden)
    - AlazMcpServer::alaz_save (learner üzerinden)
```

**Dosyalar**: Yeni `crates/alaz-intel/src/code_index.rs`, `Cargo.toml`'a `syn` dependency

---

## Faz 6: Meta-Cognition — Bilgi Sağlığı (3-4 gün)

**Neden**: Sistem ne bildiğini ve ne bilmediğini bilmeli.

### 6.1 — Knowledge Health Score

Her entity için composite skor:

```rust
pub struct KnowledgeHealth {
    /// 0.0-1.0, son erişimden bu yana geçen süreye göre
    pub freshness: f64,
    /// 0.0-1.0, Wilson score + confirmation/contradiction ratio
    pub confidence: f64,
    /// Konu başına item sayısı / ortalama
    pub coverage: f64,
    /// CTR + usage success rate
    pub usefulness: f64,
    /// Weighted average
    pub overall: f64,
}
```

### 6.2 — `alaz_health` MCP Tool

```
alaz_health(project: "Alaz")
→
📊 Bilgi Sağlığı — Alaz
═══════════════════════
Toplam: 156 item (92 pattern, 34 episode, 18 procedure, 12 core memory)

Konulara Göre:
  🟢 Search pipeline  — 23 item, freshness %95, confidence %88
  🟢 Auth/JWT         — 12 item, freshness %90, confidence %92
  🟡 Learning pipeline — 15 item, freshness %70, confidence %75
  🟡 Deploy           —  5 item, freshness %60, confidence %80
  🔴 Error handling   —  2 item, freshness %30, confidence %50

Aksiyonlar:
  ⚠️ 8 item 30+ gündür erişilmemiş → gözden geçir veya arşivle
  ⚠️ 3 procedure Wilson score < 0.2 → güvenilirlik düşük
  💡 "Error handling" konusunda bilgi boşluğu → daha fazla bilgi gerekli
```

### 6.3 — Knowledge Gap Detection

```rust
/// Bilgi boşluğu tespiti:
///
/// 1. Son N session'daki sorguları analiz et
/// 2. Düşük CTR'lı sorguları bul (soru soruldu ama tıklanmadı = cevap yok)
/// 3. RAPTOR cluster boyutlarını karşılaştır (küçük cluster = zayıf konu)
/// 4. Session'larda tekrar eden ama bilgi tabanında karşılığı olmayan
///    keyword'leri tespit et
pub struct GapDetector { ... }
```

### 6.4 — Proactive Gap Alerts

Haftalık bildirim:
```
🕳️ Tespit Edilen Bilgi Boşlukları:
  1. "rollback" — 4 session'da soruldu, 0 procedure var
     → Öneri: Deploy rollback prosedürü oluştur
  2. "monitoring" — 2 session'da soruldu, 1 zayıf item var
     → Öneri: Monitoring bilgisini güçlendir
```

**Dosyalar**: Yeni `crates/alaz-intel/src/health.rs`, `crates/alaz-intel/src/gap_detector.rs`, MCP tool

---

## Faz 7: Bilgi Evrimi + Spaced Repetition (3-4 gün)

**Neden**: Bilgi statik değil, evrilir. Unutma da doğal. İkisini yönetmek lazım.

### 7.1 — Evolution Timeline

```rust
/// Supersede zincirini geriye doğru takip et.
///
/// Returns: [(version, item, reason, date)]
/// Örnek: v3 ← v2 ← v1
pub async fn get_evolution_chain(pool: &PgPool, id: &str) -> Vec<EvolutionEntry>
```

MCP tool:
```
alaz_evolution(id: "pattern_abc")
→
📜 Bilgi Evrimi: "Deploy Pipeline Pattern"
  v1 (2026-03-15) — İlk versiyon: basit rsync
  v2 (2026-03-22) — patchelf eklendi (NixOS → Arch uyumu)
     Neden: "Binary server'da çalışmıyordu"
  v3 (2026-03-29) — systemctl restart eklendi
     Neden: "Deploy sonrası servis yeniden başlamıyordu"
  
  Trend: 2 haftada 3 iterasyon → hâlâ olgunlaşıyor
```

### 7.2 — Spaced Repetition

Ebbinghaus forgetting curve'üne göre:

```rust
/// Önemli bilgileri periyodik olarak hatırlat.
///
/// Bilgi ilk öğrenildiğinde: 1 gün, 3 gün, 7 gün, 14 gün, 30 gün aralıklarla
/// context injection'a dahil et. Her erişimde interval uzar.
pub struct SpacedRepetition {
    /// SM-2 algoritması parametreleri
    easiness_factor: f64,  // default 2.5
    interval_days: u32,
    repetitions: u32,
}
```

```sql
-- Migration: spaced repetition tracking
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_interval_days INT DEFAULT 1;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_easiness REAL DEFAULT 2.5;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_next_review TIMESTAMPTZ;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_repetitions INT DEFAULT 0;
```

### 7.3 — Review Session

Context injection'da:
```
📝 Bugün gözden geçirilecek bilgiler (3):
  1. "Wilson score formula" — son gözden geçirme: 14 gün önce
  2. "Qdrant collection naming" — son gözden geçirme: 7 gün önce  
  3. "FTS simple dictionary convention" — son gözden geçirme: 30 gün önce
```

**Dosyalar**: `crates/alaz-intel/src/spaced_repetition.rs`, `context.rs`'e ekleme, migration

---

## Bonus Özellikler (Fazlarla Paralel)

Bu özellikler herhangi bir fazdan sonra eklenebilir:

| Özellik | Bağımlılık | Efor | Açıklama |
|---------|------------|------|----------|
| `alaz_teach` — Explicit öğretme modu | Yok | 0.5 gün | "Bunu öğren: ..." ile yapılandırılmış bilgi girişi |
| Multi-project correlation | Faz 2 | 2 gün | "Bu pattern'i 3 projede kullandın" tespiti |
| Semantic diff | Faz 7.1 | 1 gün | "Geçen haftaya göre bilgi tabanında ne değişti?" |
| File watcher | Faz 4 | 1 gün | Proje dosyası değişince bilgiyi güncelle |
| Trend analysis | Faz 6 | 1 gün | "Auth bilgilerin sık değişiyor" gibi meta-gözlemler |

---

## Bağımlılık Grafiği

```
Faz 1 (Adaptive Weights) ──────────────────────┐
    │                                           │
    ▼                                           ▼
Faz 3 (Explain)                           Faz 2 (Consolidation)
                                                │
Faz 4 (Git Integration) ──→ Faz 5 (Codebase)  │
                                                │
                              ┌─────────────────┘
                              ▼
                        Faz 6 (Meta-Cognition)
                              │
                              ▼
                        Faz 7 (Evolution + Spaced Rep)
```

**Paralel gidebilecekler**:
- Faz 1 + Faz 4 (bağımsız)
- Faz 3 + Faz 2 (bağımsız)
- Bonus özellikler herhangi bir noktada

---

## Zaman Çizelgesi

| Faz | Tahmini Süre | Kümülatif |
|-----|-------------|-----------|
| Faz 1: Adaptive Weights | 3-4 gün | 1. hafta |
| Faz 2: Consolidation | 3-4 gün | 1-2. hafta |
| Faz 3: Explain | 2 gün | 2. hafta |
| Faz 4: Git Integration | 4-5 gün | 3. hafta |
| Faz 5: Codebase Awareness | 5-7 gün | 4. hafta |
| Faz 6: Meta-Cognition | 3-4 gün | 5. hafta |
| Faz 7: Evolution + SR | 3-4 gün | 5-6. hafta |
| Bonus | 5-6 gün | Paralel |

**Toplam**: ~28-36 gün (6-8 hafta)

---

## Başarı Metrikleri

Her fazdan sonra ölçülecek:

| Metrik | Mevcut | Faz 1 Sonrası | Faz 6 Sonrası |
|--------|--------|---------------|---------------|
| Arama CTR | Ölçülmüyor | %X baseline | +%20 hedef |
| Bilgi item sayısı / noise | ~300 / bilinmiyor | Baseline | -%15 noise |
| Ortalama freshness | Bilinmiyor | Baseline | >%80 hedef |
| Knowledge gap sayısı | Bilinmiyor | — | Tespit ediliyor |
| Signal weight değişim sıklığı | 0 (static) | Haftalık | Haftalık |

---

## Mimari Etkiler

### Yeni Crate'ler (önerilmez)
Mevcut 9 crate yapısı yeterli. Yeni özellikler mevcut crate'lere eklenir:
- `alaz-intel`: consolidation, health, gap_detector, spaced_repetition, code_index
- `alaz-db`: git_activity repo, signal_weights repo
- `alaz-server`: git endpoint, yeni MCP tool'lar, yeni job'lar

### Yeni Tablolar (tahmini)
- `signal_weights` — Öğrenilmiş signal ağırlıkları
- `git_activity` — Git commit/file değişiklik takibi
- `code_symbols` — AST-level symbol index
- Mevcut tablolara SR kolonları eklenmesi

### Yeni Background Job'lar
- Weight learning (haftalık)
- Consolidation (haftalık)
- Health check (günlük)
- Git polling (opsiyonel, hook tercih edilir)
- SR review scheduling (günlük)

### Yeni MCP Tool'lar
- `alaz_explain` — Arama sonucu açıklaması
- `alaz_health` — Bilgi sağlığı raporu
- `alaz_evolution` — Bilgi evrim geçmişi
- `alaz_impact` — Kod değişikliği etki analizi
- `alaz_teach` — Explicit bilgi öğretme
- `alaz_gaps` — Bilgi boşlukları raporu
