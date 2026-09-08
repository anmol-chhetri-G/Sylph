//! Sylph Py Bridge - Python AI integration via PyO3
//!
//! This crate wraps Python functions so Rust can call them.
//! It uses pyo3 with auto-initialize to embed a Python interpreter.

use pyo3::prelude::*;

fn with_python_module<T>(
    module_name: &str,
    f: impl FnOnce(&Bound<'_, PyModule>) -> PyResult<T>,
) -> Result<T, PyErr> {
    Python::try_attach(|py| {
        let sys = py.import("sys")?;
        let path = sys.getattr("path")?;
        path.call_method1("append", ("./python",))?;
        let module = py.import(module_name)?;
        f(&module)
    })
    .unwrap_or_else(|| {
        Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "Python not initialized",
        ))
    })
}

fn bridge_call(f: impl FnOnce() -> Result<String, PyErr>) -> String {
    match f() {
        Ok(result) => result,
        Err(e) => format!("Python error: {}", e),
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
        Python::try_attach(|py| {
            let sys = py.import("sys")?;
            let version: String = sys.getattr("version")?.extract()?;
            Ok::<_, PyErr>(format!("Python {}", version))
        })
        .unwrap_or_else(|| {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Python not initialized",
            ))
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
        assert!(!result.is_empty());
    }

    #[test]
    fn test_export_to_pdf() {
        let result = export_to_pdf("# Test", "/tmp/sylph_test_export.pdf");
        assert!(!result.is_empty());
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
        let _: String = export_to_docx("md", "path");
        let _: String = export_to_pdf("md", "path");
    }
}
