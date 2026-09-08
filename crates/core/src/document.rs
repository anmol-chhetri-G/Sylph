use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpanStyle {
    Bold,
    Italic,
    BoldItalic,
    Code,
    Strikethrough,
    Underline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub styles: Vec<SpanStyle>,
}

impl TextRun {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            styles: Vec::new(),
        }
    }

    pub fn styled(text: impl Into<String>, styles: Vec<SpanStyle>) -> Self {
        Self {
            text: text.into(),
            styles,
        }
    }

    pub fn plain_text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParagraphStyle {
    pub line_spacing: f32,
    pub space_before: f32,
    pub space_after: f32,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            line_spacing: 1.5,
            space_before: 0.0,
            space_after: 8.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub runs: Vec<TextRun>,
}

impl TableCell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            runs: vec![TextRun::plain(text)],
        }
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableData {
    pub rows: Vec<Vec<TableCell>>,
    pub caption: Option<String>,
    pub column_widths: Vec<f32>,
}

impl TableData {
    pub fn new(rows: usize, cols: usize) -> Self {
        let rows_data = (0..rows)
            .map(|_| (0..cols).map(|_| TableCell::plain("")).collect())
            .collect();
        Self {
            rows: rows_data,
            caption: None,
            column_widths: vec![100.0 / cols as f32; cols],
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn col_count(&self) -> usize {
        self.rows.first().map_or(0, |r| r.len())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageData {
    pub path: String,
    pub alt_text: String,
    pub width: f32,
    pub height: f32,
    pub caption: Option<String>,
}

impl ImageData {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            alt_text: String::new(),
            width: 400.0,
            height: 300.0,
            caption: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Caption {
    pub text: String,
}

impl Caption {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverPageData {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub date: String,
    pub background_image: Option<String>,
    pub logo: Option<String>,
    pub template: CoverTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CoverTemplate {
    Classic,
    Modern,
    Minimal,
    Bold,
    Academic,
}

impl CoverPageData {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            subtitle: String::new(),
            author: String::new(),
            date: chrono_free_date(),
            background_image: None,
            logo: None,
            template: CoverTemplate::Classic,
        }
    }

    pub fn with_template(template: CoverTemplate) -> Self {
        Self {
            template,
            ..Self::new()
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    pub fn with_background_image(mut self, path: impl Into<String>) -> Self {
        self.background_image = Some(path.into());
        self
    }

    pub fn with_logo(mut self, path: impl Into<String>) -> Self {
        self.logo = Some(path.into());
        self
    }
}

fn chrono_free_date() -> String {
    // Simple date string without chrono dependency
    // Format: YYYY-MM-DD
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let mut y = 1970u32;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year as u64 {
            break;
        }
        remaining -= days_in_year as u64;
        y += 1;
    }
    let mut m = 1u32;
    let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    while m <= 12 {
        let dim = if m == 2 && is_leap(y) {
            29
        } else {
            days_in_month[(m - 1) as usize] as u64
        };
        if remaining < dim {
            break;
        }
        remaining -= dim;
        m += 1;
    }
    format!("{:04}-{:02}-{:02}", y, m, remaining + 1)
}

fn is_leap(y: u32) -> bool {
    y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400))
}

impl Default for CoverPageData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Block {
    Paragraph {
        runs: Vec<TextRun>,
        style: ParagraphStyle,
    },
    Heading {
        level: u8,
        runs: Vec<TextRun>,
    },
    Image {
        data: ImageData,
    },
    Table {
        data: TableData,
    },
    Caption {
        text: String,
    },
    CoverPage {
        data: CoverPageData,
    },
    HorizontalRule,
    PageBreak,
}

impl Block {
    pub fn paragraph(text: impl Into<String>) -> Self {
        Self::Paragraph {
            runs: vec![TextRun::plain(text)],
            style: ParagraphStyle::default(),
        }
    }

    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        Self::Heading {
            level: level.clamp(1, 5),
            runs: vec![TextRun::plain(text)],
        }
    }

    pub fn image(path: impl Into<String>) -> Self {
        Self::Image {
            data: ImageData::new(path),
        }
    }

    pub fn table(rows: usize, cols: usize) -> Self {
        Self::Table {
            data: TableData::new(rows, cols),
        }
    }

    pub fn caption(text: impl Into<String>) -> Self {
        Self::Caption { text: text.into() }
    }

    pub fn cover_page() -> Self {
        Self::CoverPage {
            data: CoverPageData::new(),
        }
    }

    pub fn cover_page_with(template: CoverTemplate) -> Self {
        Self::CoverPage {
            data: CoverPageData::with_template(template),
        }
    }

    pub fn horizontal_rule() -> Self {
        Self::HorizontalRule
    }

    pub fn page_break() -> Self {
        Self::PageBreak
    }

    pub fn plain_text(&self) -> String {
        match self {
            Self::Paragraph { runs, .. } => runs.iter().map(|r| r.text.as_str()).collect(),
            Self::Heading { runs, .. } => runs.iter().map(|r| r.text.as_str()).collect(),
            Self::Caption { text } => text.clone(),
            Self::Table { data } => data
                .rows
                .iter()
                .flat_map(|row| row.iter().map(|cell| cell.text()))
                .collect::<Vec<_>>()
                .join(" "),
            Self::Image { data } => data.alt_text.clone(),
            Self::CoverPage { data } => {
                format!("{} {} {}", data.title, data.subtitle, data.author)
            }
            Self::HorizontalRule => String::new(),
            Self::PageBreak => String::new(),
        }
    }

    pub fn set_paragraph_style(&mut self, style: ParagraphStyle) {
        if let Self::Paragraph { style: s, .. } = self {
            *s = style;
        }
    }

    pub fn set_heading_level(&mut self, level: u8) {
        if let Self::Heading { level: l, .. } = self {
            *l = level.clamp(1, 5);
        }
    }

    pub fn set_caption(&mut self, cap: impl Into<String>) {
        match self {
            Self::Image { data } => data.caption = Some(cap.into()),
            Self::Table { data } => data.caption = Some(cap.into()),
            _ => {}
        }
    }

    pub fn is_cover_page(&self) -> bool {
        matches!(self, Self::CoverPage { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
}

impl PageSize {
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            PageSize::A4 => (595.0, 842.0),     // points (210mm x 297mm)
            PageSize::Letter => (612.0, 792.0), // points (8.5" x 11")
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            PageSize::A4 => "A4",
            PageSize::Letter => "Letter",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageMargins {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Default for PageMargins {
    fn default() -> Self {
        Self {
            top: 72.0, // 1 inch = 72 points
            bottom: 72.0,
            left: 72.0,
            right: 72.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub title: String,
    pub page_size: PageSize,
    pub page_margins: PageMargins,
}

impl Document {
    pub fn new() -> Self {
        Self {
            blocks: vec![Block::paragraph("")],
            title: "Untitled".to_string(),
            page_size: PageSize::A4,
            page_margins: PageMargins::default(),
        }
    }

    pub fn with_title(title: impl Into<String>) -> Self {
        Self {
            blocks: vec![Block::paragraph("")],
            title: title.into(),
            page_size: PageSize::A4,
            page_margins: PageMargins::default(),
        }
    }

    pub fn set_page_size(&mut self, size: PageSize) {
        self.page_size = size;
    }

    pub fn set_margins(&mut self, margins: PageMargins) {
        self.page_margins = margins;
    }

    pub fn page_width(&self) -> f32 {
        self.page_size.dimensions().0
    }

    pub fn page_height(&self) -> f32 {
        self.page_size.dimensions().1
    }

    pub fn content_width(&self) -> f32 {
        self.page_width() - self.page_margins.left - self.page_margins.right
    }

    pub fn content_height(&self) -> f32 {
        self.page_height() - self.page_margins.top - self.page_margins.bottom
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.len() == 1
            && matches!(&self.blocks[0], Block::Paragraph { runs, .. } if runs.len() == 1 && runs[0].text.is_empty())
    }

    pub fn insert_block(&mut self, index: usize, block: Block) {
        let idx = index.min(self.blocks.len());
        self.blocks.insert(idx, block);
    }

    pub fn remove_block(&mut self, index: usize) -> Option<Block> {
        if index < self.blocks.len() && self.blocks.len() > 1 {
            Some(self.blocks.remove(index))
        } else {
            None
        }
    }

    pub fn push_block(&mut self, block: Block) {
        self.blocks.push(block);
    }

    pub fn append_paragraph(&mut self, text: impl Into<String>) {
        self.blocks.push(Block::paragraph(text));
    }

    pub fn append_heading(&mut self, level: u8, text: impl Into<String>) {
        self.blocks.push(Block::heading(level, text));
    }

    pub fn to_plaintext(&self) -> String {
        self.blocks
            .iter()
            .map(|b| b.plain_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn from_plaintext(text: &str) -> Self {
        let blocks = text
            .split('\n')
            .map(|line| Block::paragraph(line.to_string()))
            .collect();
        Self {
            blocks,
            title: "Untitled".to_string(),
            page_size: PageSize::A4,
            page_margins: PageMargins::default(),
        }
    }

    pub fn set_cover_page(&mut self, data: CoverPageData) {
        if let Some(first) = self.blocks.first_mut() {
            if first.is_cover_page() {
                *first = Block::CoverPage { data };
            } else {
                self.blocks.insert(0, Block::CoverPage { data });
            }
        } else {
            self.blocks.push(Block::CoverPage { data });
        }
    }

    pub fn cover_page(&self) -> Option<&CoverPageData> {
        self.blocks.first().and_then(|b| {
            if let Block::CoverPage { data } = b {
                Some(data)
            } else {
                None
            }
        })
    }

    pub fn cover_page_mut(&mut self) -> Option<&mut CoverPageData> {
        self.blocks.first_mut().and_then(|b| {
            if let Block::CoverPage { data } = b {
                Some(data)
            } else {
                None
            }
        })
    }

    pub fn has_cover_page(&self) -> bool {
        self.blocks.first().is_some_and(|b| b.is_cover_page())
    }

    pub fn remove_cover_page(&mut self) {
        if self.has_cover_page() {
            self.blocks.remove(0);
        }
    }

    pub fn set_cover_background_image(&mut self, path: impl Into<String>) {
        if let Some(data) = self.cover_page_mut() {
            data.background_image = Some(path.into());
        }
    }

    pub fn set_cover_title(&mut self, title: impl Into<String>) {
        if let Some(data) = self.cover_page_mut() {
            data.title = title.into();
        }
    }

    pub fn set_cover_subtitle(&mut self, subtitle: impl Into<String>) {
        if let Some(data) = self.cover_page_mut() {
            data.subtitle = subtitle.into();
        }
    }

    pub fn set_cover_author(&mut self, author: impl Into<String>) {
        if let Some(data) = self.cover_page_mut() {
            data.author = author.into();
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TextRun ─────────────────────────────────────────────────

    #[test]
    fn test_text_run_plain() {
        let run = TextRun::plain("hello");
        assert_eq!(run.plain_text(), "hello");
        assert!(run.styles.is_empty());
        assert!(!run.is_empty());
        assert_eq!(run.len(), 5);
    }

    #[test]
    fn test_text_run_styled() {
        let run = TextRun::styled("bold text", vec![SpanStyle::Bold]);
        assert_eq!(run.text, "bold text");
        assert!(run.styles.contains(&SpanStyle::Bold));
    }

    #[test]
    fn test_text_run_empty() {
        let run = TextRun::plain("");
        assert!(run.is_empty());
        assert_eq!(run.len(), 0);
    }

    // ── TableCell ───────────────────────────────────────────────

    #[test]
    fn test_table_cell_plain() {
        let cell = TableCell::plain("hello");
        assert_eq!(cell.text(), "hello");
    }

    // ── TableData ───────────────────────────────────────────────

    #[test]
    fn test_table_data_new() {
        let table = TableData::new(3, 4);
        assert_eq!(table.row_count(), 3);
        assert_eq!(table.col_count(), 4);
        assert!(table.caption.is_none());
        assert_eq!(table.column_widths.len(), 4);
    }

    #[test]
    fn test_table_data_empty() {
        let table = TableData::new(0, 0);
        assert_eq!(table.row_count(), 0);
        assert_eq!(table.col_count(), 0);
    }

    // ── ImageData ───────────────────────────────────────────────

    #[test]
    fn test_image_data_new() {
        let img = ImageData::new("/path/to/image.png");
        assert_eq!(img.path, "/path/to/image.png");
        assert!(img.alt_text.is_empty());
        assert!(img.caption.is_none());
        assert!(img.width > 0.0);
        assert!(img.height > 0.0);
    }

    // ── Caption ─────────────────────────────────────────────────

    #[test]
    fn test_caption_new() {
        let cap = Caption::new("Figure 1");
        assert_eq!(cap.text, "Figure 1");
    }

    // ── Block ───────────────────────────────────────────────────

    #[test]
    fn test_block_paragraph() {
        let block = Block::paragraph("Hello world");
        assert_eq!(block.plain_text(), "Hello world");
    }

    #[test]
    fn test_block_heading() {
        let block = Block::heading(2, "Title");
        assert_eq!(block.plain_text(), "Title");
    }

    #[test]
    fn test_block_heading_clamps_level() {
        let block = Block::heading(10, "Too high");
        if let Block::Heading { level, .. } = block {
            assert_eq!(level, 5);
        } else {
            panic!("Expected Heading");
        }

        let block = Block::heading(0, "Too low");
        if let Block::Heading { level, .. } = block {
            assert_eq!(level, 1);
        } else {
            panic!("Expected Heading");
        }
    }

    #[test]
    fn test_block_image() {
        let block = Block::image("/test.png");
        if let Block::Image { data } = block {
            assert_eq!(data.path, "/test.png");
        } else {
            panic!("Expected Image");
        }
    }

    #[test]
    fn test_block_table() {
        let block = Block::table(2, 3);
        if let Block::Table { data } = block {
            assert_eq!(data.row_count(), 2);
            assert_eq!(data.col_count(), 3);
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn test_block_caption() {
        let block = Block::caption("Figure 1: Test");
        assert_eq!(block.plain_text(), "Figure 1: Test");
    }

    #[test]
    fn test_block_horizontal_rule() {
        let block = Block::horizontal_rule();
        assert_eq!(block.plain_text(), "");
    }

    #[test]
    fn test_block_set_caption_on_image() {
        let mut block = Block::image("/test.png");
        block.set_caption("My image");
        if let Block::Image { data } = block {
            assert_eq!(data.caption.as_deref(), Some("My image"));
        } else {
            panic!("Expected Image");
        }
    }

    #[test]
    fn test_block_set_caption_on_table() {
        let mut block = Block::table(2, 2);
        block.set_caption("Table 1");
        if let Block::Table { data } = block {
            assert_eq!(data.caption.as_deref(), Some("Table 1"));
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn test_block_set_paragraph_style() {
        let mut block = Block::paragraph("test");
        let style = ParagraphStyle {
            line_spacing: 2.0,
            space_before: 10.0,
            space_after: 20.0,
        };
        block.set_paragraph_style(style);
        if let Block::Paragraph { style: s, .. } = block {
            assert_eq!(s.line_spacing, 2.0);
            assert_eq!(s.space_before, 10.0);
            assert_eq!(s.space_after, 20.0);
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_block_set_heading_level() {
        let mut block = Block::heading(1, "test");
        block.set_heading_level(4);
        if let Block::Heading { level, .. } = block {
            assert_eq!(level, 4);
        } else {
            panic!("Expected Heading");
        }
    }

    // ── ParagraphStyle ──────────────────────────────────────────

    #[test]
    fn test_paragraph_style_default() {
        let style = ParagraphStyle::default();
        assert_eq!(style.line_spacing, 1.5);
        assert_eq!(style.space_before, 0.0);
        assert_eq!(style.space_after, 8.0);
    }

    // ── Document ────────────────────────────────────────────────

    #[test]
    fn test_document_new() {
        let doc = Document::new();
        assert_eq!(doc.block_count(), 1);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_document_with_title() {
        let doc = Document::with_title("My Doc");
        assert_eq!(doc.title, "My Doc");
    }

    #[test]
    fn test_document_insert_block() {
        let mut doc = Document::new();
        doc.insert_block(0, Block::heading(1, "Title"));
        assert_eq!(doc.block_count(), 2);
        assert_eq!(doc.blocks[0].plain_text(), "Title");
    }

    #[test]
    fn test_document_remove_block() {
        let mut doc = Document::new();
        doc.insert_block(0, Block::heading(1, "Title"));
        let removed = doc.remove_block(0);
        assert!(removed.is_some());
        assert_eq!(doc.block_count(), 1);
    }

    #[test]
    fn test_document_remove_last_block_returns_none() {
        let mut doc = Document::new();
        let removed = doc.remove_block(0);
        assert!(removed.is_none());
    }

    #[test]
    fn test_document_push_block() {
        let mut doc = Document::new();
        doc.push_block(Block::paragraph("Hello"));
        doc.push_block(Block::paragraph("World"));
        assert_eq!(doc.block_count(), 3);
    }

    #[test]
    fn test_document_append_paragraph() {
        let mut doc = Document::new();
        doc.append_paragraph("Line 1");
        doc.append_paragraph("Line 2");
        assert_eq!(doc.block_count(), 3);
    }

    #[test]
    fn test_document_append_heading() {
        let mut doc = Document::new();
        doc.append_heading(1, "Title");
        doc.append_heading(2, "Subtitle");
        assert_eq!(doc.block_count(), 3);
    }

    #[test]
    fn test_document_to_plaintext() {
        let mut doc = Document::new();
        doc.blocks = vec![Block::heading(1, "Title"), Block::paragraph("Body text")];
        let text = doc.to_plaintext();
        assert_eq!(text, "Title\nBody text");
    }

    #[test]
    fn test_document_from_plaintext() {
        let doc = Document::from_plaintext("line1\nline2\nline3");
        assert_eq!(doc.block_count(), 3);
        assert_eq!(doc.blocks[0].plain_text(), "line1");
        assert_eq!(doc.blocks[1].plain_text(), "line2");
        assert_eq!(doc.blocks[2].plain_text(), "line3");
    }

    #[test]
    fn test_document_from_empty_plaintext_keeps_editable_block() {
        let doc = Document::from_plaintext("");
        assert_eq!(doc.block_count(), 1);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_document_from_plaintext_preserves_trailing_newline() {
        let doc = Document::from_plaintext("line1\n");
        assert_eq!(doc.block_count(), 2);
        assert_eq!(doc.blocks[1].plain_text(), "");
    }

    #[test]
    fn test_document_default() {
        let doc = Document::default();
        assert!(doc.is_empty());
    }

    // ── CoverPage ───────────────────────────────────────────────

    #[test]
    fn test_cover_page_data_new() {
        let cp = CoverPageData::new();
        assert!(cp.title.is_empty());
        assert!(cp.subtitle.is_empty());
        assert!(cp.author.is_empty());
        assert!(cp.background_image.is_none());
        assert!(cp.logo.is_none());
        assert_eq!(cp.template, CoverTemplate::Classic);
    }

    #[test]
    fn test_cover_page_data_builder() {
        let cp = CoverPageData::with_template(CoverTemplate::Modern)
            .with_title("My Book")
            .with_subtitle("A Novel")
            .with_author("Author")
            .with_background_image("/img.png")
            .with_logo("/logo.png");
        assert_eq!(cp.title, "My Book");
        assert_eq!(cp.subtitle, "A Novel");
        assert_eq!(cp.author, "Author");
        assert_eq!(cp.background_image.as_deref(), Some("/img.png"));
        assert_eq!(cp.logo.as_deref(), Some("/logo.png"));
        assert_eq!(cp.template, CoverTemplate::Modern);
    }

    #[test]
    fn test_block_cover_page() {
        let block = Block::cover_page();
        assert!(block.is_cover_page());
        if let Block::CoverPage { data } = block {
            assert_eq!(data.template, CoverTemplate::Classic);
        } else {
            panic!("Expected CoverPage");
        }
    }

    #[test]
    fn test_block_cover_page_with_template() {
        let block = Block::cover_page_with(CoverTemplate::Bold);
        if let Block::CoverPage { data } = block {
            assert_eq!(data.template, CoverTemplate::Bold);
        } else {
            panic!("Expected CoverPage");
        }
    }

    #[test]
    fn test_block_cover_page_plain_text() {
        let mut cp = CoverPageData::new();
        cp.title = "Title".to_string();
        cp.subtitle = "Sub".to_string();
        cp.author = "Author".to_string();
        let block = Block::CoverPage { data: cp };
        let text = block.plain_text();
        assert!(text.contains("Title"));
        assert!(text.contains("Sub"));
        assert!(text.contains("Author"));
    }

    #[test]
    fn test_document_set_cover_page() {
        let mut doc = Document::new();
        let cp = CoverPageData::with_template(CoverTemplate::Modern).with_title("Test");
        doc.set_cover_page(cp);
        assert!(doc.has_cover_page());
        assert_eq!(doc.block_count(), 2); // cover + empty paragraph
    }

    #[test]
    fn test_document_cover_page_accessor() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new().with_title("My Title"));
        let cp = doc.cover_page().unwrap();
        assert_eq!(cp.title, "My Title");
    }

    #[test]
    fn test_document_cover_page_mut() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new());
        doc.cover_page_mut().unwrap().title = "Changed".to_string();
        assert_eq!(doc.cover_page().unwrap().title, "Changed");
    }

    #[test]
    fn test_document_has_cover_page() {
        let mut doc = Document::new();
        assert!(!doc.has_cover_page());
        doc.set_cover_page(CoverPageData::new());
        assert!(doc.has_cover_page());
    }

    #[test]
    fn test_document_remove_cover_page() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new());
        assert!(doc.has_cover_page());
        doc.remove_cover_page();
        assert!(!doc.has_cover_page());
    }

    #[test]
    fn test_document_set_cover_background_image() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new());
        doc.set_cover_background_image("/cover.jpg");
        assert_eq!(
            doc.cover_page().unwrap().background_image.as_deref(),
            Some("/cover.jpg")
        );
    }

    #[test]
    fn test_document_set_cover_title() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new());
        doc.set_cover_title("New Title");
        assert_eq!(doc.cover_page().unwrap().title, "New Title");
    }

    #[test]
    fn test_document_set_cover_subtitle() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new());
        doc.set_cover_subtitle("New Subtitle");
        assert_eq!(doc.cover_page().unwrap().subtitle, "New Subtitle");
    }

    #[test]
    fn test_document_set_cover_author() {
        let mut doc = Document::new();
        doc.set_cover_page(CoverPageData::new());
        doc.set_cover_author("New Author");
        assert_eq!(doc.cover_page().unwrap().author, "New Author");
    }

    #[test]
    fn test_cover_template_variants() {
        assert_eq!(CoverTemplate::Classic, CoverTemplate::Classic);
        assert_eq!(CoverTemplate::Modern, CoverTemplate::Modern);
        assert_eq!(CoverTemplate::Minimal, CoverTemplate::Minimal);
        assert_eq!(CoverTemplate::Bold, CoverTemplate::Bold);
        assert_eq!(CoverTemplate::Academic, CoverTemplate::Academic);
    }

    // ── Compile-time Type Checks ────────────────────────────────

    #[test]
    fn test_api_signatures_compile() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<Document>();
        assert_sync::<Document>();
        assert_send::<Block>();
        assert_send::<TextRun>();
        assert_send::<TableData>();
        assert_send::<ImageData>();
    }
}
