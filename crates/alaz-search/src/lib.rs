pub mod agentic;
pub mod cache;
pub mod classifier;
pub mod decay;
pub mod fusion;
pub mod pipeline;
pub mod proactive;
pub mod rerank;
pub mod signals;
pub mod weight_learning;

pub use cache::SearchCache;
pub use classifier::{QueryType, SearchWeights, classify_query};
pub use decay::apply_decay;
pub use fusion::{reciprocal_rank_fusion, weighted_reciprocal_rank_fusion};
pub use pipeline::SearchPipeline;
pub use proactive::{ProactiveResult, extract_keywords, proactive_search};
pub use rerank::{RerankConfig, Reranker};
