//! Near-duplicate, burst, and gap clustering. Exact-duplicate grouping lives
//! in `db` as a plain query; burst and gap clustering arrive with embeddings
//! (milestones 4-5).

pub mod burst;
pub mod near;
pub mod scoring;
