//! Pasted API keys and tokens often carry surrounding whitespace or one pair of quotes
//! (copied from a JSON file or a shell line). Strip exactly that, nothing inside.

/// `s` without surrounding whitespace and one pair of matching surrounding quotes.
pub fn clean(s: &str) -> String {
    let s = s.trim();
    for q in ['"', '\'', '`'] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.trim().to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn strips_whitespace_and_one_pair_of_quotes() {
        assert_eq!(clean("  abc123 \n"), "abc123");
        assert_eq!(clean("\"abc123\""), "abc123");
        assert_eq!(clean(" 'abc123' "), "abc123");
        assert_eq!(clean("\"  abc123  \""), "abc123");
        assert_eq!(clean("\"\"abc\"\""), "\"abc\"", "only one pair goes");
    }

    #[test]
    fn leaves_unpaired_quotes_and_inner_text_alone() {
        assert_eq!(clean("\"abc"), "\"abc");
        assert_eq!(clean("abc\""), "abc\"");
        assert_eq!(clean("Bearer x y"), "Bearer x y");
        assert_eq!(clean("\""), "\"");
        assert_eq!(clean(""), "");
    }
}
