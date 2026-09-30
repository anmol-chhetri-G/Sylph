//! Document titles.

/// The title a rename saves: trimmed, and "Untitled" rather than empty.
pub(crate) fn confirmed_title(typed: &str) -> String {
    match typed.trim() {
        "" => "Untitled".to_string(),
        title => title.to_string(),
    }
}

#[cfg(test)]
mod title_tests {
    use super::confirmed_title;

    #[test]
    fn a_rename_is_trimmed_and_never_empty() {
        assert_eq!(confirmed_title("  Report  "), "Report");
        assert_eq!(confirmed_title("   "), "Untitled");
        assert_eq!(confirmed_title(""), "Untitled");
        assert_eq!(confirmed_title("नेपाली लेख"), "नेपाली लेख");
    }
}
