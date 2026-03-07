//! Simple bracket mirroring helpers for BiDi text.
//!
//! Full Unicode mirroring is more extensive than this; for now we
//! provide basic support for common ASCII brackets and parentheses.

/// Return the mirrored counterpart for common ASCII brackets.
///
/// If `ch` does not have a known mirror, it is returned unchanged.
pub fn mirrored_bracket(ch: char) -> char {
    match ch {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        _ => ch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_parentheses() {
        assert_eq!(mirrored_bracket('('), ')');
        assert_eq!(mirrored_bracket(')'), '(');
    }

    #[test]
    fn mirrors_brackets_and_braces() {
        assert_eq!(mirrored_bracket('['), ']');
        assert_eq!(mirrored_bracket(']'), '[');
        assert_eq!(mirrored_bracket('{'), '}');
        assert_eq!(mirrored_bracket('}'), '{');
    }

    #[test]
    fn leaves_non_brackets_unchanged() {
        assert_eq!(mirrored_bracket('a'), 'a');
        let hebrew = "אב".chars().next().unwrap();
        assert_eq!(mirrored_bracket(hebrew), hebrew);
    }
}
