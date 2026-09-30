//! The command palette's commands and filtering.

/// The command palette's rows: glyph, title, hint. Titles are what
/// `SylphApp::run_palette_command` dispatches on.
pub(crate) const PALETTE_COMMANDS: [(&str, &str, &str); 8] = [
    ("H1", "Heading 1", "# + space"),
    ("H2", "Heading 2", "## + space"),
    ("☷", "Bullet list", "- + space"),
    ("1.", "Numbered list", "1. + space"),
    ("▦", "Table", "grid"),
    ("▧", "Image", "upload"),
    ("↵", "Page break", "Ctrl+Enter"),
    ("<>\u{00a0}", "Code block", "```"),
];

/// Commands that edit Markdown source, so they need Markdown mode.
pub(crate) fn palette_command_needs_markdown(title: &str) -> bool {
    matches!(
        title,
        "Heading 1" | "Heading 2" | "Bullet list" | "Numbered list" | "Code block"
    )
}

/// Indices into `PALETTE_COMMANDS` whose title contains every word of
/// `query` (case-insensitive), in list order.
pub(crate) fn palette_matches(query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    PALETTE_COMMANDS
        .iter()
        .enumerate()
        .filter(|(_, (_, title, _))| {
            let title = title.to_lowercase();
            query.split_whitespace().all(|word| title.contains(word))
        })
        .map(|(i, _)| i)
        .collect()
}

#[derive(Default)]
pub(crate) struct PaletteState {
    /// What was typed to filter the commands.
    pub(crate) query: String,
    /// Highlighted row, an index into `palette_matches(query)`.
    pub(crate) selected: usize,
}

#[cfg(test)]
mod palette_tests {
    use super::{palette_matches, PALETTE_COMMANDS};

    fn titles(query: &str) -> Vec<&'static str> {
        palette_matches(query)
            .into_iter()
            .map(|i| PALETTE_COMMANDS[i].1)
            .collect()
    }

    #[test]
    fn typing_filters_the_commands() {
        assert_eq!(titles("").len(), PALETTE_COMMANDS.len());
        assert_eq!(titles("head"), ["Heading 1", "Heading 2"]);
        assert_eq!(titles("LIST"), ["Bullet list", "Numbered list"]);
        assert_eq!(titles("num list"), ["Numbered list"]);
        assert!(titles("zzz").is_empty());
        assert!(titles("नेपाल").is_empty());
    }
}
