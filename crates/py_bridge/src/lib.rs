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
                if let Ok(paths) =
                    glob.call_method1("glob", (pattern,))?.extract::<Vec<String>>()
                {
                    for p in paths {
                        let _ = path.call_method1("append", (p,));
                    }
                }
            }
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

    #[test]
    fn test_export_rich_markdown() {
        let doc = r#"{"blocks": [
            {"Heading": {"level": 1, "runs": [{"text": "T", "styles": []}]}}
        ]}"#;
        let result = export_rich_markdown(doc, "/tmp/sylph_test_rich.md");
        assert!(result.starts_with("Exported"), "got: {result}");
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
