//! Stable item identity derived from the creation timestamp.
//!
//! An item's numeric `_id` is positional and changes on archive and restore,
//! so links cannot use it. The stable ID plays the role of a jj change ID: it
//! is a pure function of `_timestamp` (which never changes after creation),
//! computed whenever an item is read and never stored.
//!
//! The timestamp is passed through an invertible 64-bit mixer, so distinct
//! timestamps never collide and nearby timestamps get unrelated prefixes, then
//! written as 16 letters from jj's "reverse hex" alphabet (`k`..=`z`). The
//! letters-only alphabet keeps stable IDs lexically disjoint from numeric IDs.
//!
//! The mapping is part of the data contract: every `[[id]]` token written by a
//! user depends on it. Never change the mixer constants or the alphabet.

use std::collections::HashMap;

/// The 16-letter alphabet; index `n` encodes nibble value `n`.
pub const ALPHABET: &[u8; 16] = b"klmnopqrstuvwxyz";

/// Length of a full stable ID.
pub const LEN: usize = 16;

/// Derive the stable ID for an item created at `timestamp` (epoch millis).
pub fn from_timestamp(timestamp: i64) -> String {
    let mixed = mix(timestamp as u64);
    (0..LEN)
        .map(|i| {
            let nibble = (mixed >> (4 * (LEN - 1 - i))) & 0xf;
            ALPHABET[nibble as usize] as char
        })
        .collect()
}

/// The splitmix64 finalizer: a bijection on `u64` (xorshifts and odd
/// multipliers are each invertible), with good avalanche between nearby inputs.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Shortest prefix of `id` shared with neither sorted neighbour.
pub(crate) fn unique_prefix_len(prev: Option<&str>, next: Option<&str>, id: &str) -> usize {
    let common = |other: &str| {
        other
            .bytes()
            .zip(id.bytes())
            .take_while(|(a, b)| a == b)
            .count()
    };
    let shared = prev.map_or(0, common).max(next.map_or(0, common));
    (shared + 1).min(id.len())
}

fn is_alphabet_run(s: &str) -> bool {
    s.bytes().all(|b| ALPHABET.contains(&b))
}

/// True when `s` is a full stable ID: exactly 16 lowercase alphabet letters.
pub fn is_full_id(s: &str) -> bool {
    s.len() == LEN && is_alphabet_run(s)
}

/// True when `s` is a non-empty, shorter-than-full run of lowercase alphabet
/// letters, i.e. something that can only be a stable-ID prefix.
pub fn is_prefix(s: &str) -> bool {
    !s.is_empty() && s.len() < LEN && is_alphabet_run(s)
}

/// For each distinct ID, the length of its shortest prefix that no other ID in
/// the set shares. IDs that occur more than once get the full length.
pub fn shortest_unique_prefix_lengths<'a, I>(ids: I) -> HashMap<String, usize>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut sorted: Vec<&str> = ids.into_iter().collect();
    sorted.sort_unstable();

    let mut lengths = HashMap::with_capacity(sorted.len());
    for (i, id) in sorted.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| sorted[j]);
        let next = sorted.get(i + 1).copied();
        let len = if prev == Some(*id) || next == Some(*id) {
            LEN
        } else {
            unique_prefix_len(prev, next, id)
        };
        lengths.insert((*id).to_string(), len);
    }
    lengths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vectors, computed independently of this module. They freeze the
    /// derivation: if any of these change, every stored link breaks.
    #[test]
    fn golden_vectors_never_change() {
        let cases: [(i64, &str); 6] = [
            (0, "kkkkkkkkkkkkkkkk"),
            (-1, "voxkppzwzmwvvxrv"),
            (i64::MIN, "mpwmqyuprtwyutsu"),
            (i64::MAX, "puqsmuzyrtqpxyvx"),
            (1_790_000_000_000, "nuyzmslzwrsnzpqn"),
            (1_790_000_000_001, "tqqzmlsvvrtzwzqz"),
        ];
        for (ts, expected) in cases {
            assert_eq!(from_timestamp(ts), expected, "timestamp {ts}");
        }
    }

    #[test]
    fn output_is_sixteen_alphabet_letters() {
        for ts in [0, 1, 42, -7, 1_700_000_000_000, i64::MAX, i64::MIN] {
            let id = from_timestamp(ts);
            assert_eq!(id.len(), LEN);
            assert!(id.bytes().all(|b| ALPHABET.contains(&b)), "{id}");
            assert!(is_full_id(&id));
        }
    }

    #[test]
    fn distinct_timestamps_never_collide() {
        let base = 1_790_000_000_000_i64;
        let mut seen = std::collections::HashSet::new();
        for offset in 0..10_000 {
            assert!(seen.insert(from_timestamp(base + offset)));
        }
    }

    #[test]
    fn adjacent_timestamps_spread_their_first_letter() {
        let base = 1_790_000_000_000_i64;
        let differing = (0..1_000)
            .filter(|&i| {
                from_timestamp(base + i).as_bytes()[0] != from_timestamp(base + i + 1).as_bytes()[0]
            })
            .count();
        // A uniform spread differs 15 times in 16; demand most of that.
        assert!(differing > 850, "only {differing} of 1000 differ");
    }

    #[test]
    fn full_id_and_prefix_recognition() {
        assert!(is_full_id("kqzxmvtnplsrkqzx"));
        assert!(!is_full_id("kqzxmvtnplsrkqz"));
        assert!(!is_full_id("kqzxmvtnplsrkqzxk"));
        assert!(!is_full_id("KQZXMVTNPLSRKQZX"));
        assert!(!is_full_id("aqzxmvtnplsrkqzx"));

        assert!(is_prefix("k"));
        assert!(is_prefix("kqzxmvtnplsrkqz"));
        assert!(!is_prefix(""));
        assert!(!is_prefix("kqzxmvtnplsrkqzx"));
        assert!(!is_prefix("Kq"));
        assert!(!is_prefix("todo"));
        assert!(!is_prefix("12"));
    }

    #[test]
    fn shortest_unique_prefixes() {
        let ids = ["kqzaaaaaaaaaaaaa", "kqyaaaaaaaaaaaaa", "maaaaaaaaaaaaaaa"];
        let lens = shortest_unique_prefix_lengths(ids.iter().copied());
        assert_eq!(lens["kqzaaaaaaaaaaaaa"], 3);
        assert_eq!(lens["kqyaaaaaaaaaaaaa"], 3);
        assert_eq!(lens["maaaaaaaaaaaaaaa"], 1);
    }

    #[test]
    fn lone_id_gets_length_one_and_duplicates_get_full_length() {
        let lens = shortest_unique_prefix_lengths(["kqzxmvtnplsrkqzx"]);
        assert_eq!(lens["kqzxmvtnplsrkqzx"], 1);

        let dup = shortest_unique_prefix_lengths(["kqzxmvtnplsrkqzx", "kqzxmvtnplsrkqzx", "mkkk"]);
        assert_eq!(dup["kqzxmvtnplsrkqzx"], LEN);
    }
}
