//! The size envelope taskbook is designed for.
//!
//! Storage is a JSON file per item set: every operation reads, parses and
//! rewrites the whole file, and sync uploads the whole set. That is fine at
//! personal scale and is not meant to scale past it. Features may do O(n)
//! work per operation and may rely on these sizes (for example, accepting
//! that items created in the same millisecond on two devices share a stable
//! ID). Stores beyond the envelope are unsupported, not cases to design
//! around.

/// Active items the design supports.
pub const SUPPORTED_ACTIVE_ITEMS: usize = 1024;

/// Archived items the design supports. The archive grows over the years,
/// and every create reads it to keep creation timestamps unique.
pub const SUPPORTED_ARCHIVED_ITEMS: usize = 4096;

/// Active items we recommend staying under.
pub const RECOMMENDED_ACTIVE_ITEMS: usize = 512;
