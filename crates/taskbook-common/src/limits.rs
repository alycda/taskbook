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

/// How a store compares with the envelope, once it is past the recommended
/// active count or a supported limit. Informational: nothing is blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeNotice {
    pub active: usize,
    pub archived: usize,
}

impl SizeNotice {
    /// A notice for a store with these counts, or `None` while it is within
    /// the recommendation and the supported archive size.
    pub fn check(active: usize, archived: usize) -> Option<Self> {
        let notice = Self { active, archived };
        (notice.active_over_recommended() || notice.archive_over_supported()).then_some(notice)
    }

    pub fn active_over_recommended(&self) -> bool {
        self.active > RECOMMENDED_ACTIVE_ITEMS
    }

    pub fn archive_over_supported(&self) -> bool {
        self.archived > SUPPORTED_ARCHIVED_ITEMS
    }

    /// One line naming each limit the store is past.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.active > SUPPORTED_ACTIVE_ITEMS {
            parts.push(format!(
                "{} active items, over the {SUPPORTED_ACTIVE_ITEMS} supported \
                 ({RECOMMENDED_ACTIVE_ITEMS} recommended)",
                self.active
            ));
        } else if self.active_over_recommended() {
            parts.push(format!(
                "{} active items, over the {RECOMMENDED_ACTIVE_ITEMS} recommended",
                self.active
            ));
        }
        if self.archive_over_supported() {
            parts.push(format!(
                "{} archived items, over the {SUPPORTED_ARCHIVED_ITEMS} supported",
                self.archived
            ));
        }
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_notice_at_the_limits() {
        assert_eq!(
            SizeNotice::check(RECOMMENDED_ACTIVE_ITEMS, SUPPORTED_ARCHIVED_ITEMS),
            None
        );
    }

    #[test]
    fn active_past_the_recommendation() {
        let notice = SizeNotice::check(RECOMMENDED_ACTIVE_ITEMS + 1, 0).unwrap();
        assert_eq!(
            notice.summary(),
            "513 active items, over the 512 recommended"
        );
    }

    #[test]
    fn active_past_the_supported_size() {
        let notice = SizeNotice::check(SUPPORTED_ACTIVE_ITEMS + 1, 0).unwrap();
        assert_eq!(
            notice.summary(),
            "1025 active items, over the 1024 supported (512 recommended)"
        );
    }

    #[test]
    fn archive_past_the_supported_size_alone() {
        let notice = SizeNotice::check(10, SUPPORTED_ARCHIVED_ITEMS + 1).unwrap();
        assert!(!notice.active_over_recommended());
        assert_eq!(
            notice.summary(),
            "4097 archived items, over the 4096 supported"
        );
    }

    #[test]
    fn both_limits_are_named() {
        let notice = SizeNotice::check(600, 5000).unwrap();
        assert_eq!(
            notice.summary(),
            "600 active items, over the 512 recommended; 5000 archived items, over the 4096 supported"
        );
    }
}
