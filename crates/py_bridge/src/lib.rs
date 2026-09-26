//! Sylph Py Bridge - Python AI integration via PyO3
//!
//! This crate wraps Python functions so Rust can call them.
//! It uses pyo3 with auto-initialize to embed a Python interpreter.

use pyo3::prelude::*;

fn python_search_paths() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(dir) = std::env::var("SYLPH_PYTHON_DIR") {
        out.push(dir);
    }
    // Build-time repo location: works no matter where the binary or test
    // binary is launched from (cwd-relative paths break otherwise).
    if let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
    {
        out.push(root.join("python").to_string_lossy().into_owned());
    }
    out.push("./python".to_string());
    // `cargo test -p sylph-py-bridge` runs with cwd = crates/py_bridge,
    // so "./python" alone misses. Walk up looking for python/export.py.
    let mut dir = std::env::current_dir().ok();
    for _ in 0..6 {
        let Some(d) = dir else { break };
        let cand = d.join("python").join("export.py");
        if cand.is_file() {
            out.push(d.join("python").to_string_lossy().into_owned());
            break;
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    out
}

/// Site-packages of the repo's own `.venv`, located from the crate's
/// build-time directory. The embedded interpreter does not honor
/// `$VIRTUAL_ENV` unless the shell exported it, so without this plain
/// `cargo test` misses python-docx/fpdf and the export proofs fail.
fn repo_venv_site_packages() -> Vec<String> {
    let mut out = Vec::new();
    let mut root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    for _ in 0..6 {
        if root.join("python").join("export.py").is_file() {
            let venv = root.join(".venv");
            // Unix layout: .venv/lib/pythonX.Y/site-packages
            if let Ok(lib) = venv.join("lib").read_dir() {
                for entry in lib.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("python") {
                        let sp = entry.path().join("site-packages");
                        if sp.is_dir() {
                            out.push(sp.to_string_lossy().into_owned());
                        }
                    }
                }
            }
            // Windows layout: .venv/Lib/site-packages
            let win = venv.join("Lib").join("site-packages");
            if win.is_dir() {
                out.push(win.to_string_lossy().into_owned());
            }
            break;
        }
        if !root.pop() {
            break;
        }
    }
    out
}

fn with_python_module<T>(
    module_name: &str,
    f: impl FnOnce(&Bound<'_, PyModule>) -> PyResult<T>,
) -> Result<T, PyErr> {
    // Blocking attach (not try_attach): concurrent callers — e.g. parallel
    // `cargo test` threads — wait for the GIL instead of failing with
    // "Python not initialized".
    Python::attach(|py| {
        let sys = py.import("sys")?;
        let path = sys.getattr("path")?;
        for cand in python_search_paths() {
            path.call_method1("append", (cand,))?;
        }
        // Honor an active venv (PEP-668 systems like Arch forbid system
        // pip installs, so export deps live in .venv). The embedded
        // interpreter does not pick up $VIRTUAL_ENV automatically.
        if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
            let pattern = format!("{}/lib/python*/site-packages", venv);
            if let Ok(glob) = py.import("glob") {
                if let Ok(paths) = glob
                    .call_method1("glob", (pattern,))?
                    .extract::<Vec<String>>()
                {
                    for p in paths {
                        let _ = path.call_method1("append", (p,));
                    }
                }
            }
        }
        // …and fall back to the repo's own .venv, so tests and CLI exports
        // work without requiring `source .venv/bin/activate` first.
        for p in repo_venv_site_packages() {
            let _ = path.call_method1("append", (p,));
        }
        let module = py.import(module_name)?;
        f(&module)
    })
}

fn bridge_call(f: impl FnOnce() -> Result<String, PyErr>) -> String {
    match f() {
        Ok(result) => result,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("No module named") {
                format!(
                    "Python error: {}. Hint: python3 -m venv .venv && .venv/bin/pip install -r python/requirements.txt, then run with the venv active",
                    msg
                )
            } else {
                format!("Python error: {}", msg)
            }
        }
    }
}

/// Call the Python summarize function.
pub fn summarize_text(text: &str) -> String {
    bridge_call(|| {
        with_python_module("ai", |module| {
            let result = module.call_method1("summarize", (text,))?;
            result.extract::<String>()
        })
    })
}

/// Check if Python bridge is working.
pub fn ping() -> String {
    bridge_call(|| {
        Python::attach(|py| {
            let sys = py.import("sys")?;
            let version: String = sys.getattr("version")?.extract()?;
            Ok::<_, PyErr>(format!("Python {}", version))
        })
    })
}

/// Export document to DOCX format.
pub fn export_to_docx(text: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module("export", |module| {
            let result = module.call_method1("markdown_to_docx", (text, output_path))?;
            let success: bool = result.extract()?;
            Ok(if success {
                format!("Exported to {}", output_path)
            } else {
                "Export failed".to_string()
            })
        })
    })
}

/// Export document to PDF format.
pub fn export_to_pdf(text: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module("export", |module| {
            let result = module.call_method1("markdown_to_pdf", (text, output_path))?;
            let success: bool = result.extract()?;
            Ok(if success {
                format!("Exported to {}", output_path)
            } else {
                "Export failed".to_string()
            })
        })
    })
}

/// Rewrite text in a different style via AI.
pub fn rewrite_text(text: &str, style: &str) -> String {
    bridge_call(|| {
        with_python_module("ai", |module| {
            let result = module.call_method1("rewrite", (text, style))?;
            result.extract::<String>()
        })
    })
}

/// Chat with document context via AI.
pub fn chat_with_doc(question: &str, context: &str) -> String {
    bridge_call(|| {
        with_python_module("ai", |module| {
            let result = module.call_method1("chat_with_doc", (question, context))?;
            result.extract::<String>()
        })
    })
}

/// Export rich document (JSON-serialized) to DOCX format.
pub fn export_rich_docx(doc_json: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module("export", |module| {
            let result = module.call_method1("rich_docx", (doc_json, output_path))?;
            let success: bool = result.extract()?;
            Ok(if success {
                format!("Exported to {}", output_path)
            } else {
                "Export failed".to_string()
            })
        })
    })
}

/// Export rich document (JSON-serialized) to PDF format.
pub fn export_rich_pdf(doc_json: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module("export", |module| {
            let result = module.call_method1("rich_pdf", (doc_json, output_path))?;
            let success: bool = result.extract()?;
            Ok(if success {
                format!("Exported to {}", output_path)
            } else {
                "Export failed".to_string()
            })
        })
    })
}

/// Export rich document (JSON-serialized) to Markdown file.
pub fn export_rich_markdown(doc_json: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module("export", |module| {
            let result = module.call_method1("rich_markdown", (doc_json, output_path))?;
            let success: bool = result.extract()?;
            Ok(if success {
                format!("Exported to {}", output_path)
            } else {
                "Export failed".to_string()
            })
        })
    })
}

/// Save image data to file and return the path.
pub fn save_image(data: &[u8], output_path: &str) -> String {
    match std::fs::write(output_path, data) {
        Ok(()) => output_path.to_string(),
        Err(e) => format!("Failed to save image: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping() {
        let result = ping();
        assert!(!result.is_empty());
        assert!(result.starts_with("Python "));
    }

    #[test]
    fn test_summarize_text() {
        let result =
            summarize_text("This is a test document with enough words to summarize properly.");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_rewrite_text() {
        let result = rewrite_text("Hello world", "formal");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_chat_with_doc() {
        let result = chat_with_doc("What is this about?", "This is a test document.");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_export_to_docx() {
        let result = export_to_docx("# Test", "/tmp/sylph_test_export.docx");
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    fn test_export_to_pdf() {
        let result = export_to_pdf("# Test", "/tmp/sylph_test_export.pdf");
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    fn test_export_rich_pdf() {
        // Cover title carries an em-dash: locks the _pdf_safe fix for
        // cover-page cell() calls (latin-1 core fonts).
        let doc = r#"{"blocks": [
            {"CoverPage": {"data": {"title": "T — cover", "subtitle": "", "author": "", "date": "", "background_image": null, "logo": null, "template": "Classic"}}},
            {"Heading": {"level": 1, "runs": [{"text": "Hi — “q”", "styles": []}]}},
            {"Paragraph": {"runs": [
                {"text": "a", "styles": []},
                {"text": "b", "styles": ["BoldItalic"]}
            ], "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}},
            "PageBreak",
            {"Paragraph": {"runs": [{"text": "p2", "styles": []}], "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}
        ]}"#;
        let result = export_rich_pdf(doc, "/tmp/sylph_test_rich.pdf");
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    fn test_export_rich_docx() {
        // Missing-file image + caption locks the placeholder-caption path
        // (and the WD_ALIGN_PARAGRAPH / RGBColor imports it needs).
        let doc = r#"{"blocks": [
            {"Heading": {"level": 2, "runs": [{"text": "H", "styles": ["BoldItalic"]}]}},
            {"Image": {"data": {"path": "fixtures/definitely-missing.png", "alt_text": "x", "width": 400.0, "height": 300.0, "caption": "Fig — cap"}}},
            {"Table": {"data": {"rows": [
                [{"runs": [{"text": "A", "styles": ["Bold"]}]}],
                [{"runs": [{"text": "1", "styles": []}]}]
            ], "caption": null, "column_widths": [100.0]}}},
            "PageBreak"
        ]}"#;
        let result = export_rich_docx(doc, "/tmp/sylph_test_rich.docx");
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    /// One XML part of an exported .docx, read with Python's zipfile.
    fn docx_part(path: &str, part: &str) -> String {
        with_python_module("zipfile", |zipfile| {
            let archive = zipfile.call_method1("ZipFile", (path,))?;
            let xml = archive.call_method1("read", (part,))?;
            xml.call_method1("decode", ("utf-8",))?.extract::<String>()
        })
        .expect("readable docx")
    }

    fn docx_document_xml(path: &str) -> String {
        docx_part(path, "word/document.xml")
    }

    /// The `<w:style>` element with `style_id` from a .docx's styles.xml.
    fn docx_style(path: &str, style_id: &str) -> String {
        let styles = docx_part(path, "word/styles.xml");
        let start = styles
            .find(&format!(r#"w:styleId="{style_id}""#))
            .unwrap_or_else(|| panic!("no style {style_id}"));
        let end = start + styles[start..].find("</w:style>").expect("closed style");
        styles[start..end].to_string()
    }

    fn pdf_has(path: &str, needle: &str) -> bool {
        let pdf = std::fs::read(path).expect("readable pdf");
        pdf.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn test_rich_exports_use_the_documents_page_setup() {
        // Letter with narrow (36 pt) margins: python-docx's own template
        // is Letter with 1.25" sides and fpdf's default is A4 with 10 mm,
        // so both must come from the document instead.
        let doc = r#"{"page_size": "Letter", "landscape": false,
            "page_margins": {"top": 36.0, "bottom": 36.0, "left": 36.0, "right": 36.0},
            "blocks": [{"Paragraph": {"runs": [{"text": "x", "styles": []}],
                "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}]}"#;
        let r = export_rich_pdf(doc, "/tmp/sylph_test_setup.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(
            "/tmp/sylph_test_setup.pdf",
            "/MediaBox [0 0 612.00 792.00]"
        ));

        let r = export_rich_docx(doc, "/tmp/sylph_test_setup.docx");
        assert!(r.starts_with("Exported"), "docx: {r}");
        let xml = docx_document_xml("/tmp/sylph_test_setup.docx");
        // Twips: 1 pt = 20. Letter is 612 × 792 pt; margins 36 pt.
        assert!(xml.contains(r#"<w:pgSz w:w="12240" w:h="15840""#), "{xml}");
        for side in ["top", "right", "bottom", "left"] {
            assert!(
                xml.contains(&format!(r#"w:{side}="720""#)),
                "{side} in {xml}"
            );
        }
    }

    #[test]
    fn test_rich_exports_default_to_a4_and_honour_landscape() {
        // No page keys at all: Sylph's defaults (A4, 1-inch margins), not
        // the libraries' — the editor shows A4 for a new document.
        let body = r#""blocks": [{"Paragraph": {"runs": [{"text": "x", "styles": []}],
            "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}]"#;
        let portrait = format!("{{{body}}}");
        let r = export_rich_docx(&portrait, "/tmp/sylph_test_a4.docx");
        assert!(r.starts_with("Exported"), "docx: {r}");
        let xml = docx_document_xml("/tmp/sylph_test_a4.docx");
        assert!(xml.contains(r#"<w:pgSz w:w="11900" w:h="16840""#), "{xml}");
        assert!(xml.contains(r#"w:left="1440""#), "{xml}");
        let r = export_rich_pdf(&portrait, "/tmp/sylph_test_a4.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(
            "/tmp/sylph_test_a4.pdf",
            "/MediaBox [0 0 595.00 842.00]"
        ));

        let landscape = format!(r#"{{"page_size": "A4", "landscape": true, {body}}}"#);
        let r = export_rich_pdf(&landscape, "/tmp/sylph_test_a4_landscape.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(
            "/tmp/sylph_test_a4_landscape.pdf",
            "/MediaBox [0 0 842.00 595.00]"
        ));
        let r = export_rich_docx(&landscape, "/tmp/sylph_test_a4_landscape.docx");
        assert!(r.starts_with("Exported"), "docx: {r}");
        let xml = docx_document_xml("/tmp/sylph_test_a4_landscape.docx");
        assert!(
            xml.contains(r#"w:w="16840" w:h="11900" w:orient="landscape""#),
            "{xml}"
        );
    }

    /// A heading and a paragraph with a bold run, set in `body_font`.
    fn typography_doc(body_font: &str) -> String {
        format!(
            r#"{{"body_font": "{body_font}", "body_font_size": 13.0, "blocks": [
                {{"Heading": {{"level": 1, "runs": [{{"text": "Title", "styles": []}}]}}}},
                {{"Paragraph": {{"runs": [
                    {{"text": "plain ", "styles": []}},
                    {{"text": "bold", "styles": ["Bold"]}}
                ], "style": {{"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}}}}
            ]}}"#
        )
    }

    #[test]
    fn test_rich_docx_uses_the_documents_typography() {
        // The canvas shows body_font at body_font_size and headings at
        // 28/22/18/16/14/12 pt; the template's Calibri 11 and theme
        // heading fonts must not replace them.
        let r = export_rich_docx(&typography_doc("Noto Serif"), "/tmp/sylph_test_type.docx");
        assert!(r.starts_with("Exported"), "docx: {r}");
        let normal = docx_style("/tmp/sylph_test_type.docx", "Normal");
        assert!(normal.contains(r#"w:ascii="Noto Serif""#), "{normal}");
        assert!(normal.contains(r#"<w:sz w:val="26"/>"#), "13 pt: {normal}");
        let h1 = docx_style("/tmp/sylph_test_type.docx", "Heading1");
        assert!(h1.contains(r#"w:ascii="Noto Serif""#), "{h1}");
        // Word prefers theme fonts over w:ascii, so they must be gone.
        assert!(!h1.contains("w:asciiTheme"), "{h1}");
        assert!(h1.contains(r#"<w:sz w:val="56"/>"#), "28 pt: {h1}");
        assert!(
            h1.contains(r#"w:before="240" w:after="120""#),
            "12/6 pt: {h1}"
        );
    }

    /// Text drawn on the pages of a PDF. fpdf2 deflates page streams, so
    /// they are inflated with Python's zlib before searching.
    fn pdf_text(path: &str) -> String {
        const INFLATE: &std::ffi::CStr = cr#"
import re, zlib

def text(path):
    data = open(path, 'rb').read()
    out = []
    for m in re.finditer(rb'stream\r?\n(.*?)\r?\nendstream', data, re.S):
        try:
            out.append(zlib.decompress(m.group(1)))
        except zlib.error:
            out.append(m.group(1))
    return b'\n'.join(out).decode('latin-1')
"#;
        with_python_module("zlib", |zlib| {
            let helper = PyModule::from_code(zlib.py(), INFLATE, c"pdf_text.py", c"pdf_text")?;
            helper.call_method1("text", (path,))?.extract::<String>()
        })
        .expect("readable pdf")
    }

    #[test]
    fn test_pdf_table_cells_wrap_instead_of_being_cut() {
        // Cells were cut to 50 characters (60 in the plain-markdown path):
        // a silent content drop. A long cell's last word must be drawn.
        let long = format!("{}TAILWORD", "alpha ".repeat(30));
        let doc = format!(
            r#"{{"blocks": [{{"Table": {{"data": {{"rows": [
                [{{"runs": [{{"text": "Header", "styles": []}}]}}],
                [{{"runs": [{{"text": "{long}", "styles": []}}]}}]
            ], "caption": null, "column_widths": [100.0]}}}}}}]}}"#
        );
        let r = export_rich_pdf(&doc, "/tmp/sylph_test_table_wrap.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let text = pdf_text("/tmp/sylph_test_table_wrap.pdf");
        assert!(text.contains("Header"), "header row drawn");
        assert!(text.contains("TAILWORD"), "rich table cell cut short");

        let markdown = format!("| H |\n|---|\n| {long} |\n");
        let r = export_to_pdf(&markdown, "/tmp/sylph_test_table_wrap_md.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let text = pdf_text("/tmp/sylph_test_table_wrap_md.pdf");
        assert!(text.contains("TAILWORD"), "markdown table cell cut short");
    }

    #[test]
    fn test_rich_pdf_maps_the_typeface_to_a_core_font() {
        // fpdf2 embeds only the 14 core fonts: serif faces map to Times,
        // sans faces stay Helvetica (the page-number footer is always
        // Helvetica-Oblique, so check the body faces only).
        let r = export_rich_pdf(&typography_doc("Noto Serif"), "/tmp/sylph_test_serif.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(
            "/tmp/sylph_test_serif.pdf",
            "/BaseFont /Times-Roman"
        ));
        assert!(pdf_has(
            "/tmp/sylph_test_serif.pdf",
            "/BaseFont /Times-Bold"
        ));
        assert!(!pdf_has(
            "/tmp/sylph_test_serif.pdf",
            "/BaseFont /Helvetica-Bold"
        ));

        let r = export_rich_pdf(&typography_doc("Noto Sans"), "/tmp/sylph_test_sans.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(
            "/tmp/sylph_test_sans.pdf",
            "/BaseFont /Helvetica-Bold"
        ));
        assert!(!pdf_has(
            "/tmp/sylph_test_sans.pdf",
            "/BaseFont /Times-Roman"
        ));
    }

    #[test]
    fn test_export_rich_markdown() {
        let doc = r#"{"blocks": [
            {"Heading": {"level": 1, "runs": [{"text": "T", "styles": []}]}}
        ]}"#;
        let result = export_rich_markdown(doc, "/tmp/sylph_test_rich.md");
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    fn test_export_rich_kitchen_sink_blocks() {
        // Every markdown-typed block shape (List/Quote/CodeBlock/Link)
        // must survive all three renderers — and the markdown output is
        // read back to prove no content was silently dropped.
        let doc = r#"{"blocks": [
            {"Heading": {"level": 1, "runs": [{"text": "Title", "styles": [{"Link": "https://example.com/h"}]}]}},
            {"Paragraph": {"runs": [
                {"text": "see ", "styles": []},
                {"text": "nested", "styles": ["Bold", {"Link": "https://example.com/b"}]},
                {"text": " now", "styles": [{"Link": "https://example.com/b"}]}
            ], "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}},
            {"List": {"items": [
                {"level": 0, "ordered": false, "checked": null, "runs": [{"text": "bullet", "styles": []}]},
                {"level": 1, "ordered": false, "checked": true, "runs": [{"text": "task", "styles": []}]},
                {"level": 0, "ordered": true, "checked": null, "runs": [{"text": "first", "styles": []}]},
                {"level": 0, "ordered": true, "checked": null, "runs": [{"text": "second", "styles": []}]}
            ]}},
            {"Quote": {"level": 2, "runs": [{"text": "quoted", "styles": ["Italic"]}]}},
            {"CodeBlock": {"language": "rust", "text": "let x = **literal**;"}},
            {"Table": {"data": {"rows": [
                [{"runs": [{"text": "Hdr", "styles": []}]},
                 {"runs": [{"text": "Two", "styles": []}]}],
                [{"runs": [{"text": "bold", "styles": ["Bold"]}]},
                 {"runs": [{"text": "site", "styles": [{"Link": "https://example.com/c"}]}]}]
            ], "caption": null, "column_widths": [50.0, 50.0]}}},
            "HorizontalRule"
        ]}"#;

        let r = export_rich_docx(doc, "/tmp/sylph_test_kitchen.docx");
        assert!(r.starts_with("Exported"), "docx: {r}");
        let r = export_rich_pdf(doc, "/tmp/sylph_test_kitchen.pdf");
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let r = export_rich_markdown(doc, "/tmp/sylph_test_kitchen.md");
        assert!(r.starts_with("Exported"), "md: {r}");

        let md = std::fs::read_to_string("/tmp/sylph_test_kitchen.md").unwrap();
        for probe in [
            "[Title](https://example.com/h)",
            "[**nested** now](https://example.com/b)", // grouped runs, one link
            "- bullet",
            "- [x] task",
            "1. first",
            "2. second",
            ">> *quoted*",
            "```rust",
            "let x = **literal**;",
            // Table cell styles must survive re-export — flattening cells
            // to plain text would silently drop bold/links in tables.
            "| Hdr | Two |",
            "| **bold** | [site](https://example.com/c) |",
            "---",
        ] {
            assert!(md.contains(probe), "missing {probe:?} in:\n{md}");
        }
    }
    #[test]
    fn test_bridge_call_error_handling() {
        let result = bridge_call(|| Err(pyo3::exceptions::PyRuntimeError::new_err("test error")));
        assert!(result.contains("Python error"));
        assert!(result.contains("test error"));
    }

    #[test]
    fn test_bridge_call_success() {
        let result = bridge_call(|| Ok("success".to_string()));
        assert_eq!(result, "success");
    }

    #[test]
    fn test_api_signatures_compile() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<String>();
        assert_sync::<String>();

        // Verify all public functions exist and have correct signatures
        let _: String = ping();
        let _: String = summarize_text("test");
        let _: String = rewrite_text("test", "formal");
        let _: String = chat_with_doc("q", "ctx");
        let _: String = export_to_docx("md", "/tmp/sylph_test_sig.docx");
        let _: String = export_to_pdf("md", "/tmp/sylph_test_sig.pdf");
    }
}
