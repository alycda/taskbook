//! Item id grammar shared by the CLI and the TUI command line.
//!
//! Ids are always bare numbers (`3`). The `@` sigil always names a board,
//! so `@3` is never an id — it is rejected with a hint, because taskbook
//! 1.x used `@<id>` for some commands.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// No id was given where at least one is required.
    Missing,
    /// The token is not a bare number.
    Invalid(String),
    /// The token is `@<digits>`: the 1.x id syntax.
    AtPrefixed(String),
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::Missing => write!(f, "no item id given"),
            IdError::Invalid(token) => write!(f, "invalid item id '{token}' (ids are numbers)"),
            IdError::AtPrefixed(token) => write!(
                f,
                "invalid item id '{token}': ids are bare numbers and @ names a board; use `{}`",
                &token[1..]
            ),
        }
    }
}

impl std::error::Error for IdError {}

fn is_at_id(token: &str) -> bool {
    token
        .strip_prefix('@')
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Parse a single id token.
pub fn parse_id(token: &str) -> Result<u64, IdError> {
    if is_at_id(token) {
        return Err(IdError::AtPrefixed(token.to_string()));
    }
    token
        .parse::<u64>()
        .map_err(|_| IdError::Invalid(token.to_string()))
}

/// Parse a list where every token must be an id. Fails on an empty list.
pub fn parse_ids<S: AsRef<str>>(tokens: &[S]) -> Result<Vec<u64>, IdError> {
    if tokens.is_empty() {
        return Err(IdError::Missing);
    }
    tokens.iter().map(|t| parse_id(t.as_ref())).collect()
}

/// Split `<id>... <rest>...`: the leading run of bare-number tokens are ids,
/// everything from the first non-number on is returned as the rest.
///
/// Fails when there is no leading id; if the first token is `@<digits>` the
/// error carries the 1.x-syntax hint.
pub fn split_leading_ids<S: AsRef<str>>(tokens: &[S]) -> Result<(Vec<u64>, &[S]), IdError> {
    let count = tokens
        .iter()
        .take_while(|t| t.as_ref().parse::<u64>().is_ok())
        .count();
    if count == 0 {
        return Err(match tokens.first() {
            Some(t) if is_at_id(t.as_ref()) => IdError::AtPrefixed(t.as_ref().to_string()),
            Some(t) => IdError::Invalid(t.as_ref().to_string()),
            None => IdError::Missing,
        });
    }
    let ids = tokens[..count]
        .iter()
        .map(|t| t.as_ref().parse::<u64>().expect("checked above"))
        .collect();
    Ok((ids, &tokens[count..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_id_accepts_bare_numbers() {
        assert_eq!(parse_id("3"), Ok(3));
    }

    #[test]
    fn test_parse_id_rejects_at_prefix_with_hint() {
        let err = parse_id("@3").unwrap_err();
        assert_eq!(err, IdError::AtPrefixed("@3".into()));
        assert!(err.to_string().contains("use `3`"));
    }

    #[test]
    fn test_parse_id_rejects_garbage() {
        assert_eq!(parse_id("foo"), Err(IdError::Invalid("foo".into())));
        assert_eq!(parse_id("@foo"), Err(IdError::Invalid("@foo".into())));
        assert_eq!(parse_id("-1"), Err(IdError::Invalid("-1".into())));
    }

    #[test]
    fn test_parse_ids_is_strict() {
        assert_eq!(parse_ids(&["1", "2"]), Ok(vec![1, 2]));
        assert_eq!(parse_ids(&["1", "x"]), Err(IdError::Invalid("x".into())));
        assert_eq!(parse_ids::<&str>(&[]), Err(IdError::Missing));
    }

    #[test]
    fn test_split_leading_ids() {
        let tokens = ["1", "2", "@work", "3"];
        let (ids, rest) = split_leading_ids(&tokens).unwrap();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(rest, &["@work", "3"]);
    }

    #[test]
    fn test_split_leading_ids_requires_an_id() {
        assert_eq!(
            split_leading_ids(&["@3", "@work"]).unwrap_err(),
            IdError::AtPrefixed("@3".into())
        );
        assert_eq!(
            split_leading_ids(&["work"]).unwrap_err(),
            IdError::Invalid("work".into())
        );
        assert_eq!(
            split_leading_ids::<&str>(&[]).unwrap_err(),
            IdError::Missing
        );
    }
}
