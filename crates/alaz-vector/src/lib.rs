pub mod client;
pub mod colbert;
pub mod dense;

pub use client::{COLLECTION_COLBERT, COLLECTION_TEXT, QdrantManager};
pub use colbert::ColbertOps;
pub use dense::DenseVectorOps;
