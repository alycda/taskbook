//! Inline `[[…]]` links between items.
//!
//! A link is a `[[<full stable ID>]]` token in an item's text (task
//! description, note title, note body); the text is the only place links are
//! stored. When the TUI saves text, references the user *added in this edit*
//! as `[[12]]` (numeric ID) or `[[kq]]` (stable-ID prefix) are rewritten to full
//! tokens, and unresolvable ones reject the save.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use crate::models::StorageItem;
use crate::stable_id;

/// How the text between `[[` and `]]` is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// Exactly 16 lowercase stable-ID letters: a link.
    Full,
    /// Digits only: a numeric-ID reference, resolved on save.
    Numeric,
    /// 1-15 lowercase stable-ID letters: a prefix reference, resolved on save.
    Prefix,
    /// Anything else, including uppercase or other letters: plain text.
    Plain,
}

/// A closed `[[…]]` span within a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte range of the whole span, brackets included.
    pub range: Range<usize>,
    /// Byte range of the inner text.
    pub inner: Range<usize>,
    pub kind: SpanKind,
}

/// Find every closed `[[…]]` span. Spans never contain a newline, so a note
/// title and body joined by `\n` cannot produce a span across the two.
pub fn scan(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = 0;
    while let Some(offset) = text[i..].find("[[") {
        let start = i + offset;
        let inner_start = start + 2;
        let Some(close) = text[inner_start..].find("]]") else {
            break;
        };
        let inner = &text[inner_start..inner_start + close];
        if let Some(nested) = inner.rfind("[[") {
            // A later `[[` opens the span that this `]]` closes.
            i = inner_start + nested;
            continue;
        }
        if let Some(newline) = inner.find('\n') {
            i = inner_start + newline + 1;
            continue;
        }
        let end = inner_start + close + 2;
        spans.push(Span {
            range: start..end,
            inner: inner_start..inner_start + close,
            kind: classify(inner),
        });
        i = end;
    }
    spans
}

fn classify(inner: &str) -> SpanKind {
    if stable_id::is_full_id(inner) {
        SpanKind::Full
    } else if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
        SpanKind::Numeric
    } else if stable_id::is_prefix(inner) {
        SpanKind::Prefix
    } else {
        SpanKind::Plain
    }
}

/// Lookups the save-time rewrite needs over the active items.
#[derive(Debug, Default)]
pub struct Resolver {
    by_numeric: HashMap<u64, String>,
    ids: Vec<String>,
    divergent: HashSet<String>,
}

impl Resolver {
    pub fn from_items<'a, I>(items: I) -> Self
    where
        I: IntoIterator<Item = &'a StorageItem>,
    {
        let mut by_numeric = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for item in items {
            let id = item.stable_id();
            by_numeric.insert(item.id(), id.clone());
            *counts.entry(id).or_default() += 1;
        }
        let divergent = counts
            .iter()
            .filter(|(_, &n)| n > 1)
            .map(|(id, _)| id.clone())
            .collect();
        let mut ids: Vec<String> = counts.into_keys().collect();
        ids.sort_unstable();
        Self {
            by_numeric,
            ids,
            divergent,
        }
    }

    /// Distinct active stable IDs starting with `prefix`, in sorted order.
    pub fn ids_with_prefix(&self, prefix: &str) -> &[String] {
        let start = self.ids.partition_point(|id| id.as_str() < prefix);
        let len = self.ids[start..]
            .iter()
            .take_while(|id| id.starts_with(prefix))
            .count();
        &self.ids[start..start + len]
    }

    /// The stable ID of the active item with this numeric ID.
    pub fn stable_id_for(&self, numeric: u64) -> Option<&str> {
        self.by_numeric.get(&numeric).map(String::as_str)
    }

    fn resolve(&self, kind: SpanKind, inner: &str) -> Result<String, RejectReason> {
        let id = match kind {
            SpanKind::Numeric => inner
                .parse::<u64>()
                .ok()
                .and_then(|n| self.stable_id_for(n))
                .ok_or(RejectReason::NotFound)?,
            SpanKind::Prefix => match self.ids_with_prefix(inner) {
                [] => return Err(RejectReason::NotFound),
                [one] => one.as_str(),
                many => return Err(RejectReason::Ambiguous(many.len())),
            },
            SpanKind::Full | SpanKind::Plain => unreachable!("only references are resolved"),
        };
        if self.is_divergent(id) {
            Err(RejectReason::Divergent)
        } else {
            Ok(id.to_string())
        }
    }

    /// True when more than one active item has this stable ID.
    pub fn is_divergent(&self, id: &str) -> bool {
        self.divergent.contains(id)
    }
}

/// Why a reference added in this edit could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    NotFound,
    Ambiguous(usize),
    Divergent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// The span as written, e.g. `[[99]]`.
    pub reference: String,
    pub reason: RejectReason,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            RejectReason::NotFound => write!(f, "{} not found", self.reference),
            RejectReason::Ambiguous(n) => write!(f, "{} matches {} items", self.reference, n),
            RejectReason::Divergent => write!(f, "{} is divergent", self.reference),
        }
    }
}

/// The text to store, plus each reference that was rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub text: String,
    /// `(reference as written, full stable ID it now links to)`.
    pub linked: Vec<(String, String)>,
}

/// Rewrite the numeric and prefix references `draft` adds relative to
/// `baseline` (the stored text when the edit began; empty for a create).
///
/// For each span text, the first k occurrences in `draft` (k = its count in
/// `baseline`) are pre-existing and stay verbatim; later ones are added.
/// Full tokens and plain spans are never changed or rejected.
pub fn rewrite_on_save(
    baseline: &str,
    draft: &str,
    resolver: &Resolver,
) -> Result<Rewrite, Vec<Rejection>> {
    let is_reference = |kind| matches!(kind, SpanKind::Numeric | SpanKind::Prefix);

    let mut baseline_counts: HashMap<&str, usize> = HashMap::new();
    for span in scan(baseline).into_iter().filter(|s| is_reference(s.kind)) {
        *baseline_counts.entry(&baseline[span.range]).or_default() += 1;
    }

    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut text = String::with_capacity(draft.len());
    let mut copied_to = 0;
    let mut linked = Vec::new();
    let mut rejections = Vec::new();

    for span in scan(draft).into_iter().filter(|s| is_reference(s.kind)) {
        let reference = &draft[span.range.clone()];
        let occurrence = seen.entry(reference).or_default();
        *occurrence += 1;
        if *occurrence <= baseline_counts.get(reference).copied().unwrap_or(0) {
            continue;
        }

        match resolver.resolve(span.kind, &draft[span.inner.clone()]) {
            Ok(id) => {
                text.push_str(&draft[copied_to..span.range.start]);
                text.push_str("[[");
                text.push_str(&id);
                text.push_str("]]");
                copied_to = span.range.end;
                linked.push((reference.to_string(), id));
            }
            Err(reason) => rejections.push(Rejection {
                reference: reference.to_string(),
                reason,
            }),
        }
    }

    if !rejections.is_empty() {
        return Err(rejections);
    }
    text.push_str(&draft[copied_to..]);
    Ok(Rewrite { text, linked })
}

/// Full stable IDs that `text` links to, deduplicated, in order of first
/// appearance.
pub fn link_targets(text: &str) -> Vec<String> {
    let mut targets: Vec<String> = Vec::new();
    for span in scan(text).into_iter().filter(|s| s.kind == SpanKind::Full) {
        let id = &text[span.inner];
        if !targets.iter().any(|t| t == id) {
            targets.push(id.to_string());
        }
    }
    targets
}

/// What a link's target resolves to among the active items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Active { numeric: u64 },
    Divergent,
    Missing,
}

/// One piece of rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    /// A resolved link: the target's current title, with tokens inside that
    /// title shown as short stable IDs (one level, so cycles cannot recurse).
    Link {
        id: String,
        title: String,
    },
    Broken {
        id: String,
    },
    Divergent {
        id: String,
    },
}

/// Outgoing links, backlinks, display prefixes and rendering over the active
/// items, rebuilt whenever the item set changes.
#[derive(Debug, Default)]
pub struct LinkIndex {
    resolver: Resolver,
    titles: HashMap<String, String>,
    numeric_by_id: HashMap<String, u64>,
    prefix_lens: HashMap<String, usize>,
    outgoing: HashMap<u64, Vec<String>>,
    inbound: HashMap<String, Vec<u64>>,
}

impl LinkIndex {
    pub fn build<'a, I>(items: I) -> Self
    where
        I: IntoIterator<Item = &'a StorageItem>,
    {
        let items: Vec<&StorageItem> = items.into_iter().collect();
        let resolver = Resolver::from_items(items.iter().copied());

        let mut titles = HashMap::new();
        let mut numeric_by_id = HashMap::new();
        let mut outgoing = HashMap::new();
        let mut inbound: HashMap<String, Vec<u64>> = HashMap::new();
        for item in &items {
            let own_id = item.stable_id();
            titles.insert(own_id.clone(), item.description().to_string());
            numeric_by_id.insert(own_id.clone(), item.id());

            let mut targets = link_targets(item.description());
            if let Some(body) = item.note_body() {
                for id in link_targets(body) {
                    if !targets.contains(&id) {
                        targets.push(id);
                    }
                }
            }
            for id in targets.iter().filter(|id| **id != own_id) {
                inbound.entry(id.clone()).or_default().push(item.id());
            }
            outgoing.insert(item.id(), targets);
        }
        for sources in inbound.values_mut() {
            sources.sort_unstable();
            sources.dedup();
        }

        let prefix_lens =
            stable_id::shortest_unique_prefix_lengths(resolver.ids.iter().map(String::as_str));

        Self {
            resolver,
            titles,
            numeric_by_id,
            prefix_lens,
            outgoing,
            inbound,
        }
    }

    pub fn resolver(&self) -> &Resolver {
        &self.resolver
    }

    pub fn target(&self, id: &str) -> Target {
        if self.resolver.is_divergent(id) {
            return Target::Divergent;
        }
        self.numeric_by_id
            .get(id)
            .map_or(Target::Missing, |&numeric| Target::Active { numeric })
    }

    /// Stable IDs the active item `numeric` links to.
    pub fn outgoing(&self, numeric: u64) -> &[String] {
        self.outgoing.get(&numeric).map_or(&[], Vec::as_slice)
    }

    /// Active items (numeric IDs, ascending) whose text links to `id`,
    /// excluding the item that holds `id` itself. Works for IDs with no active
    /// item, such as an archived one.
    pub fn backlinks(&self, id: &str) -> &[u64] {
        self.inbound.get(id).map_or(&[], Vec::as_slice)
    }

    /// Length of the shortest prefix that tells `id` apart from every active
    /// item's ID (computed against the active set plus `id` itself).
    pub fn prefix_len(&self, id: &str) -> usize {
        if let Some(&len) = self.prefix_lens.get(id) {
            return len;
        }
        let ids = &self.resolver.ids;
        let at = ids.partition_point(|other| other.as_str() < id);
        let common = |other: &String| {
            other
                .bytes()
                .zip(id.bytes())
                .take_while(|(a, b)| a == b)
                .count()
        };
        let shared = at
            .checked_sub(1)
            .map_or(0, |i| common(&ids[i]))
            .max(ids.get(at).map_or(0, common));
        (shared + 1).min(id.len())
    }

    /// `id` cut to its shortest unique prefix.
    pub fn short_id<'a>(&self, id: &'a str) -> &'a str {
        &id[..self.prefix_len(id).min(id.len())]
    }

    /// Split `text` into plain text and link segments.
    pub fn render(&self, text: &str) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut copied_to = 0;
        for span in scan(text).into_iter().filter(|s| s.kind == SpanKind::Full) {
            if span.range.start > copied_to {
                segments.push(Segment::Text(text[copied_to..span.range.start].to_string()));
            }
            let id = text[span.inner.clone()].to_string();
            segments.push(match self.target(&id) {
                Target::Active { .. } => Segment::Link {
                    title: self.flat_title(&id),
                    id,
                },
                Target::Divergent => Segment::Divergent { id },
                Target::Missing => Segment::Broken { id },
            });
            copied_to = span.range.end;
        }
        if copied_to < text.len() {
            segments.push(Segment::Text(text[copied_to..].to_string()));
        }
        segments
    }

    /// A target's title with its own tokens shown as short IDs.
    fn flat_title(&self, id: &str) -> String {
        let title = self.titles.get(id).map_or("", String::as_str);
        let mut flat = String::with_capacity(title.len());
        let mut copied_to = 0;
        for span in scan(title).into_iter().filter(|s| s.kind == SpanKind::Full) {
            flat.push_str(&title[copied_to..span.range.start]);
            flat.push_str(self.short_id(&title[span.inner.clone()]));
            copied_to = span.range.end;
        }
        flat.push_str(&title[copied_to..]);
        flat
    }
}

/// Separator used to diff a note's title and body as one text, so a reference
/// moved between the two is not treated as added. It never occurs in a span's
/// reference forms, so no reference can straddle the two fields.
const NOTE_FIELD_SEPARATOR: char = '\u{1e}';

/// Join a note's title and body for [`rewrite_on_save`].
pub fn join_note(title: &str, body: Option<&str>) -> String {
    match body {
        Some(body) => format!("{title}{NOTE_FIELD_SEPARATOR}{body}"),
        None => title.to_string(),
    }
}

/// Split text produced from [`join_note`] back into title and body.
pub fn split_note(text: &str) -> (String, Option<String>) {
    match text.split_once(NOTE_FIELD_SEPARATOR) {
        Some((title, body)) => (title.to_string(), Some(body.to_string())),
        None => (text.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Note, Task};

    fn task(id: u64, ts: i64) -> StorageItem {
        let mut t = Task::new(id, format!("task {id}"), vec!["My Board".to_string()], 1);
        t.timestamp = ts;
        StorageItem::Task(t)
    }

    fn note(id: u64, ts: i64) -> StorageItem {
        let mut n = Note::new(id, format!("note {id}"), vec!["My Board".to_string()]);
        n.timestamp = ts;
        StorageItem::Note(n)
    }

    /// Find a timestamp whose stable ID starts with `prefix`, at or after `from`.
    fn ts_with_prefix(prefix: &str, from: i64) -> i64 {
        (from..)
            .find(|&ts| stable_id::from_timestamp(ts).starts_with(prefix))
            .unwrap()
    }

    fn kinds(text: &str) -> Vec<(String, SpanKind)> {
        scan(text)
            .into_iter()
            .map(|s| (text[s.range.clone()].to_string(), s.kind))
            .collect()
    }

    #[test]
    fn scan_classifies_spans() {
        let full = stable_id::from_timestamp(1);
        let text = format!("a [[12]] b [[kq]] c [[{full}]] d [[todo]] e [[KQZ]] f [[]] g [[12a]]");
        assert_eq!(
            kinds(&text),
            vec![
                ("[[12]]".to_string(), SpanKind::Numeric),
                ("[[kq]]".to_string(), SpanKind::Prefix),
                (format!("[[{full}]]"), SpanKind::Full),
                ("[[todo]]".to_string(), SpanKind::Plain),
                ("[[KQZ]]".to_string(), SpanKind::Plain),
                ("[[]]".to_string(), SpanKind::Plain),
                ("[[12a]]".to_string(), SpanKind::Plain),
            ]
        );
    }

    #[test]
    fn scan_ignores_unclosed_and_newline_spans() {
        assert!(scan("see [[12").is_empty());
        assert!(scan("title [[\n12]] body").is_empty());
        // The later `[[` starts the span.
        assert_eq!(
            kinds("[[ [[12]]"),
            vec![("[[12]]".to_string(), SpanKind::Numeric)]
        );
    }

    #[test]
    fn scan_handles_multibyte_text() {
        let text = "café [[12]] — ünïcode [[kq]]";
        let found = kinds(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "[[12]]");
    }

    /// Covers AE1.
    #[test]
    fn numeric_reference_rewrites_and_unknowns_reject() {
        let a = ts_with_prefix("kq", 0);
        let b = ts_with_prefix("kq", a + 1);
        let items = [task(12, 1_000), task(3, a), note(4, b)];
        let r = Resolver::from_items(items.iter());

        let ok = rewrite_on_save("", "see [[12]]", &r).unwrap();
        let full = stable_id::from_timestamp(1_000);
        assert_eq!(ok.text, format!("see [[{full}]]"));
        assert_eq!(ok.linked, vec![("[[12]]".to_string(), full)]);

        let err = rewrite_on_save("", "see [[99]]", &r).unwrap_err();
        assert_eq!(err[0].to_string(), "[[99]] not found");

        let err = rewrite_on_save("", "see [[kq]]", &r).unwrap_err();
        assert_eq!(err[0].to_string(), "[[kq]] matches 2 items");
    }

    #[test]
    fn unique_prefix_rewrites() {
        let ts = ts_with_prefix("mz", 0);
        let items = [task(1, ts)];
        let r = Resolver::from_items(items.iter());
        let ok = rewrite_on_save("", "re [[mz]]", &r).unwrap();
        assert_eq!(ok.text, format!("re [[{}]]", stable_id::from_timestamp(ts)));
    }

    /// Covers AE3.
    #[test]
    fn pre_existing_numeric_text_stays_plain() {
        let items = [task(12, 1_000)];
        let r = Resolver::from_items(items.iter());
        let ok = rewrite_on_save("see [[12]]", "see [[12]] typo-fixed", &r).unwrap();
        assert_eq!(ok.text, "see [[12]] typo-fixed");
        assert!(ok.linked.is_empty());
    }

    /// Covers AE2, plus R10 for newly added tokens.
    #[test]
    fn full_tokens_are_kept_whatever_their_target() {
        let r = Resolver::from_items(std::iter::empty());
        let missing = stable_id::from_timestamp(42);
        let existing = format!("x [[{missing}]]");
        assert_eq!(
            rewrite_on_save(&existing, &existing, &r).unwrap().text,
            existing
        );

        let added = format!("y [[{missing}]]");
        assert_eq!(rewrite_on_save("y", &added, &r).unwrap().text, added);
    }

    #[test]
    fn only_occurrences_beyond_the_baseline_count_are_added() {
        let items = [task(12, 1_000)];
        let r = Resolver::from_items(items.iter());
        let full = stable_id::from_timestamp(1_000);
        let ok = rewrite_on_save("[[12]]", "[[12]] and [[12]]", &r).unwrap();
        assert_eq!(ok.text, format!("[[12]] and [[{full}]]"));
    }

    #[test]
    fn moving_a_span_between_title_and_body_is_not_an_addition() {
        let r = Resolver::from_items(std::iter::empty());
        let ok = rewrite_on_save("title\nbody [[99]]", "title [[99]]\nbody", &r).unwrap();
        assert_eq!(ok.text, "title [[99]]\nbody");
    }

    #[test]
    fn plain_spans_are_never_rejected() {
        let r = Resolver::from_items(std::iter::empty());
        for text in [
            "[[todo]]", "[[bin]]", "[[12a]]", "[[ ]]", "[[KQZ]]", "[[Work]]",
        ] {
            assert_eq!(rewrite_on_save("", text, &r).unwrap().text, text);
        }
    }

    #[test]
    fn divergent_targets_reject() {
        let ts = ts_with_prefix("pq", 0);
        let items = [task(1, ts), note(2, ts)];
        let r = Resolver::from_items(items.iter());
        let id = stable_id::from_timestamp(ts);
        assert!(r.is_divergent(&id));

        let err = rewrite_on_save("", "[[pq]]", &r).unwrap_err();
        assert_eq!(err[0].to_string(), "[[pq]] is divergent");
        let err = rewrite_on_save("", "[[1]]", &r).unwrap_err();
        assert_eq!(err[0].reason, RejectReason::Divergent);
    }

    #[test]
    fn unclosed_reference_is_left_as_text() {
        let r = Resolver::from_items(std::iter::empty());
        assert_eq!(
            rewrite_on_save("", "see [[12", &r).unwrap().text,
            "see [[12"
        );
    }

    #[test]
    fn retry_with_the_same_baseline_rejects_again() {
        let r = Resolver::from_items(std::iter::empty());
        assert!(rewrite_on_save("", "a [[99]]", &r).is_err());
        assert!(rewrite_on_save("", "a [[99]] edited", &r).is_err());
    }

    #[test]
    fn note_fields_round_trip_through_a_rewrite() {
        let items = [task(12, 1_000)];
        let r = Resolver::from_items(items.iter());
        let baseline = join_note("Title", Some("body [[12]]"));
        let draft = join_note("Title [[12]]", Some("body"));
        let (title, body) = split_note(&rewrite_on_save(&baseline, &draft, &r).unwrap().text);
        assert_eq!(title, "Title [[12]]");
        assert_eq!(body.as_deref(), Some("body"));

        let draft = join_note("New", None);
        assert_eq!(split_note(&draft), ("New".to_string(), None));
    }

    #[test]
    fn rewrite_never_panics_on_odd_input() {
        let r = Resolver::from_items(std::iter::empty());
        for text in [
            "[[",
            "]]",
            "[[]",
            "[[[[]]]]",
            "é[[é]]é",
            "[[\u{1e}12]]",
            "[[1]]]]",
            "]][[",
        ] {
            let _ = rewrite_on_save(text, text, &r);
            let _ = rewrite_on_save("", text, &r);
        }
    }

    #[test]
    fn every_rejection_is_reported() {
        let r = Resolver::from_items(std::iter::empty());
        let err = rewrite_on_save("", "[[98]] [[99]] [[mm]]", &r).unwrap_err();
        assert_eq!(err.len(), 3);
    }
    // ----- link index -----

    fn titled_task(id: u64, ts: i64, desc: String) -> StorageItem {
        let mut t = Task::new(id, desc, vec!["My Board".to_string()], 1);
        t.timestamp = ts;
        StorageItem::Task(t)
    }

    fn token(ts: i64) -> String {
        format!("[[{}]]", stable_id::from_timestamp(ts))
    }

    #[test]
    fn outgoing_and_backlinks_dedupe_and_exclude_self() {
        let (a, b) = (1_000, 2_000);
        let items = [
            titled_task(1, a, format!("A {} {} {}", token(b), token(b), token(a))),
            titled_task(2, b, "B".to_string()),
        ];
        let idx = LinkIndex::build(items.iter());
        let (ida, idb) = (stable_id::from_timestamp(a), stable_id::from_timestamp(b));

        assert_eq!(idx.outgoing(1), [idb.clone(), ida.clone()]);
        assert_eq!(idx.backlinks(&idb), [1]);
        assert!(idx.backlinks(&ida).is_empty());
    }

    #[test]
    fn link_to_missing_id_is_broken_but_still_findable_by_id() {
        let missing = stable_id::from_timestamp(9_999);
        let items = [titled_task(1, 1_000, format!("A [[{missing}]]"))];
        let idx = LinkIndex::build(items.iter());

        assert_eq!(idx.target(&missing), Target::Missing);
        assert_eq!(idx.backlinks(&missing), [1]);
        assert_eq!(
            idx.render(&format!("A [[{missing}]]")),
            vec![
                Segment::Text("A ".to_string()),
                Segment::Broken { id: missing }
            ]
        );
    }

    /// Covers AE5 (index half).
    #[test]
    fn archiving_and_restoring_a_target_toggles_its_links() {
        let t = 5_000;
        let linking = |id| titled_task(id, 1_000 + id as i64, format!("see {}", token(t)));
        let target = titled_task(7, t, "Target".to_string());
        let tid = stable_id::from_timestamp(t);

        let without = [linking(1), linking(2), linking(3)];
        let idx = LinkIndex::build(without.iter());
        assert_eq!(idx.target(&tid), Target::Missing);
        assert_eq!(idx.backlinks(&tid), [1, 2, 3]);

        let with: Vec<StorageItem> = without.iter().cloned().chain([target]).collect();
        let idx = LinkIndex::build(with.iter());
        assert_eq!(idx.target(&tid), Target::Active { numeric: 7 });
        assert_eq!(idx.backlinks(&tid), [1, 2, 3]);
    }

    #[test]
    fn shared_ids_are_divergent() {
        let ts = 3_000;
        let items = [titled_task(1, ts, "x".into()), note(2, ts)];
        let idx = LinkIndex::build(items.iter());
        let id = stable_id::from_timestamp(ts);
        assert_eq!(idx.target(&id), Target::Divergent);
        assert_eq!(
            idx.render(&format!("[[{id}]]")),
            vec![Segment::Divergent { id }]
        );
    }

    #[test]
    fn rendering_substitutes_one_level_only() {
        let (a, b) = (1_000, 2_000);
        let items = [
            titled_task(1, a, format!("see {}", token(b))),
            titled_task(2, b, format!("re {}", token(a))),
        ];
        let idx = LinkIndex::build(items.iter());
        let ida = stable_id::from_timestamp(a);
        let idb = stable_id::from_timestamp(b);

        let rendered = idx.render(&format!("x {}", token(a)));
        assert_eq!(
            rendered,
            vec![
                Segment::Text("x ".to_string()),
                Segment::Link {
                    id: ida,
                    title: format!("see {}", idx.short_id(&idb)),
                },
            ]
        );
    }

    #[test]
    fn prefix_lengths_cover_active_and_outside_ids() {
        let a = ts_with_prefix("kq", 0);
        let b = ts_with_prefix("kq", a + 1);
        let c = ts_with_prefix("z", 0);
        let items = [task(1, a), task(2, b), task(3, c)];
        let idx = LinkIndex::build(items.iter());
        let ida = stable_id::from_timestamp(a);

        assert!(idx.prefix_len(&ida) >= 3);
        assert_eq!(idx.prefix_len(&stable_id::from_timestamp(c)), 1);
        // An archived ID sharing "kq" needs at least 3 letters too.
        let outside = "kqkkkkkkkkkkkkkk";
        assert!(idx.prefix_len(outside) >= 3);
        assert!(idx.short_id(outside).starts_with("kq"));
    }

    #[test]
    fn link_targets_in_order_of_first_appearance() {
        let text = format!("{} {} {} [[12]]", token(2), token(1), token(2));
        assert_eq!(
            link_targets(&text),
            vec![stable_id::from_timestamp(2), stable_id::from_timestamp(1)]
        );
    }

    #[test]
    fn note_bodies_contribute_links() {
        let mut n = Note::new_with_body(
            1,
            "N".into(),
            Some(format!("body {}", token(2_000))),
            vec![],
        );
        n.timestamp = 1_000;
        let items = [StorageItem::Note(n), titled_task(2, 2_000, "T".into())];
        let idx = LinkIndex::build(items.iter());
        assert_eq!(idx.backlinks(&stable_id::from_timestamp(2_000)), [1]);
    }

    #[test]
    fn index_build_scales_to_thousands_of_items() {
        let items: Vec<StorageItem> = (0..5_000)
            .map(|i| {
                titled_task(
                    i + 1,
                    1_000 + i as i64,
                    format!("item {} {}", i, token(1_000 + (i as i64 + 1) % 5_000)),
                )
            })
            .collect();
        let start = std::time::Instant::now();
        let idx = LinkIndex::build(items.iter());
        assert_eq!(idx.backlinks(&stable_id::from_timestamp(1_000)).len(), 1);
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
