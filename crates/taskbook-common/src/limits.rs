//! The size envelope taskbook is designed for.
//!
//! taskbook is a task board for a person: someone reads it, triages it and
//! keeps up with it. That purpose, not storage performance, sets the bound.
//! A few hundred active items is already more than one person can keep up
//! with; the supported limits sit above that as headroom, and they apply to
//! agent use (MCP) too.
//!
//! Features should handle what a person using the board can produce, and no
//! more. Scenarios only reachable at machine scale (thousands of active
//! items, bulk writes within one millisecond, very large archives) are
//! unsupported, not edge cases to design for. That is why, for example,
//! two items created in the same millisecond on two devices may share a
//! stable ID.
//!
//! The JSON storage fits this: every operation reads, parses and rewrites
//! the whole file, and sync uploads every item, which is fast at this size
//! (measured numbers in the README's "Limits" section).

/// Active items we recommend staying under: the human-scale board.
pub const RECOMMENDED_ACTIVE_ITEMS: usize = 512;

/// Active items supported, as headroom above the recommendation.
pub const SUPPORTED_ACTIVE_ITEMS: usize = 1024;

/// Archived items supported. The archive grows over the years, and every
/// create reads it to keep creation timestamps unique.
pub const SUPPORTED_ARCHIVED_ITEMS: usize = 4096;
