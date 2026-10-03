//! Sylph Py Bridge - Python AI integration via PyO3
//!
//! This crate wraps Python functions so Rust can call them.
//! It uses pyo3 with auto-initialize to embed a Python interpreter.

use pyo3::prelude::*;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Which directories a build may trust with Python code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BuildMode {
    /// Development: the repo's `python/`, `SYLPH_PYTHON_DIR`, the repo
    /// `.venv` and an active `$VIRTUAL_ENV` are allowed too.
    Debug,
    /// Installed: only `<exe dir>/../lib/sylph/python`.
    Release,
}

impl BuildMode {
    fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Debug
        } else {
            Self::Release
        }
    }
}

/// The environment `python_dirs` may consult. Only debug builds use it.
#[derive(Clone, Debug, Default)]
pub struct PythonEnv {
    /// The repo root (debug builds only; release builds never embed it).
    pub repo: Option<PathBuf>,
    /// `$SYLPH_PYTHON_DIR`.
    pub sylph_python_dir: Option<PathBuf>,
    /// `$VIRTUAL_ENV`.
    pub virtual_env: Option<PathBuf>,
}

impl PythonEnv {
    fn from_process() -> Self {
        Self {
            repo: repo_root(),
            sylph_python_dir: std::env::var_os("SYLPH_PYTHON_DIR").map(PathBuf::from),
            virtual_env: std::env::var_os("VIRTUAL_ENV").map(PathBuf::from),
        }
    }
}

/// The repo root, baked in at build time for debug builds only: a release
/// binary must not carry (or trust) the builder's source tree.
#[cfg(debug_assertions)]
fn repo_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

#[cfg(not(debug_assertions))]
fn repo_root() -> Option<PathBuf> {
    None
}

/// Directories to put in front of `sys.path`, most trusted first: the
/// directory holding the `sylph_py` package, then (debug only) virtualenv
/// site-packages. Never the launch cwd or its ancestors: Python code
/// planted next to a document must not run (CWE-427). Group- or
/// world-writable directories are refused for the same reason.
pub fn python_dirs(mode: BuildMode, exe: &Path, env: &PythonEnv) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match mode {
        BuildMode::Release => {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("../lib/sylph/python"));
            }
        }
        BuildMode::Debug => {
            candidates.extend(env.sylph_python_dir.clone());
            if let Some(repo) = &env.repo {
                candidates.push(repo.join("python"));
            }
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("../lib/sylph/python"));
            }
            if let Some(repo) = &env.repo {
                candidates.extend(site_packages(&repo.join(".venv")));
            }
            if let Some(venv) = &env.virtual_env {
                candidates.extend(site_packages(venv));
            }
        }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        let Ok(dir) = candidate.canonicalize() else {
            continue;
        };
        if dir.is_dir() && !writable_by_others(&dir) && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// The folder of bundled fonts the PDF export embeds, from the same
/// trusted places as the code: `<exe dir>/../share/sylph/fonts` when
/// installed, and the repo's `assets/fonts` in debug builds.
pub fn fonts_dir(mode: BuildMode, exe: &Path, env: &PythonEnv) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if mode == BuildMode::Debug {
        if let Some(repo) = &env.repo {
            candidates.push(repo.join("assets/fonts"));
        }
    }
    if let Some(dir) = exe.parent() {
        candidates.push(dir.join("../share/sylph/fonts"));
    }
    candidates
        .into_iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .find(|dir| dir.is_dir() && !writable_by_others(dir))
}

/// `site-packages` of a virtualenv: `lib/pythonX.Y/site-packages` on Unix,
/// `Lib/site-packages` on Windows.
fn site_packages(venv: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(lib) = venv.join("lib").read_dir() {
        let mut dirs: Vec<PathBuf> = lib
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("python"))
            .map(|entry| entry.path().join("site-packages"))
            .collect();
        dirs.sort();
        out.extend(dirs);
    }
    out.push(venv.join("Lib").join("site-packages"));
    out
}

/// True when the directory or its parent is group- or world-writable:
/// anyone else who can write there can swap in their own code.
#[cfg(unix)]
fn writable_by_others(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    [Some(dir), dir.parent()].into_iter().flatten().any(|d| {
        d.metadata()
            .map(|meta| meta.permissions().mode() & 0o022 != 0)
            .unwrap_or(true)
    })
}

#[cfg(not(unix))]
fn writable_by_others(_dir: &Path) -> bool {
    false
}

/// Put `python_dirs` in front of `sys.path`, once per process. Appending
/// on every call (as before) grew `sys.path` forever and let anything
/// already on it shadow Sylph's modules.
fn install_python_dirs(py: Python<'_>) -> PyResult<()> {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    // The GIL is held for the whole install, so no other thread can be
    // inside it at the same time.
    let result = INSTALLED.get_or_init(|| {
        let install = || -> PyResult<()> {
            let exe = std::env::current_exe().unwrap_or_default();
            let dirs = python_dirs(BuildMode::current(), &exe, &PythonEnv::from_process());
            let path = py.import("sys")?.getattr("path")?;
            for dir in dirs.iter().rev() {
                path.call_method1("insert", (0, dir.to_string_lossy().into_owned()))?;
            }
            Ok(())
        };
        install().map_err(|e| e.to_string())
    });
    result
        .clone()
        .map_err(pyo3::exceptions::PyRuntimeError::new_err)
}

fn with_python_module<T>(
    module_name: &str,
    f: impl FnOnce(&Bound<'_, PyModule>) -> PyResult<T>,
) -> Result<T, PyErr> {
    // Blocking attach (not try_attach): concurrent callers — e.g. parallel
    // `cargo test` threads — wait for the GIL instead of failing with
    // "Python not initialized".
    Python::attach(|py| {
        install_python_dirs(py)?;
        let module = py.import(module_name)?;
        if module_name == EXPORT_MODULE {
            static FONTS: OnceLock<Option<String>> = OnceLock::new();
            let fonts = FONTS.get_or_init(|| {
                let exe = std::env::current_exe().unwrap_or_default();
                fonts_dir(BuildMode::current(), &exe, &PythonEnv::from_process())
                    .map(|dir| dir.to_string_lossy().into_owned())
            });
            module.call_method1("configure", (fonts.clone(),))?;
        }
        f(&module)
    })
}

const EXPORT_MODULE: &str = "sylph_py.export";

/// The bridge's message for a finished PDF export: where it went, plus
/// the warnings the exporter returned (characters no bundled font has).
fn pdf_export_message(warnings: Vec<String>, output_path: &str) -> String {
    if warnings.is_empty() {
        format!("Exported to {}", output_path)
    } else {
        format!(
            "Exported to {}. Warning: {}",
            output_path,
            warnings.join("; ")
        )
    }
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
        with_python_module("sylph_py.ai", |module| {
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
        with_python_module(EXPORT_MODULE, |module| {
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
        with_python_module(EXPORT_MODULE, |module| {
            let result = module.call_method1("markdown_to_pdf", (text, output_path))?;
            Ok(pdf_export_message(result.extract()?, output_path))
        })
    })
}

/// Chat with document context via AI.
pub fn chat_with_doc(question: &str, context: &str) -> String {
    bridge_call(|| {
        with_python_module("sylph_py.ai", |module| {
            let result = module.call_method1("chat_with_doc", (question, context))?;
            result.extract::<String>()
        })
    })
}

/// Export rich document (JSON-serialized) to DOCX format.
pub fn export_rich_docx(doc_json: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module(EXPORT_MODULE, |module| {
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
        with_python_module(EXPORT_MODULE, |module| {
            let result = module.call_method1("rich_pdf", (doc_json, output_path))?;
            Ok(pdf_export_message(result.extract()?, output_path))
        })
    })
}

/// Export rich document (JSON-serialized) to Markdown file.
pub fn export_rich_markdown(doc_json: &str, output_path: &str) -> String {
    bridge_call(|| {
        with_python_module(EXPORT_MODULE, |module| {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A file path in this test run's own directory (keyed by process id),
    /// so parallel or repeated runs never share fixed /tmp paths.
    fn tmp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("sylph-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test temp dir");
        dir.join(name).to_string_lossy().into_owned()
    }

    // ── Python search path (CWE-427) ──────────────────────────────

    /// A fresh directory tree for one path test.
    fn tree(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sylph-bridge-paths-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// An installed layout: `<root>/bin/sylph` next to
    /// `<root>/lib/sylph/python`, plus a repo and venvs that exist too.
    fn installed_layout(name: &str) -> (PathBuf, PathBuf, PythonEnv) {
        let root = tree(name);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lib/sylph/python/sylph_py")).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("python")).unwrap();
        std::fs::create_dir_all(repo.join(".venv/lib/python3.13/site-packages")).unwrap();
        let venv = root.join("venv");
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages")).unwrap();
        let env = PythonEnv {
            repo: Some(repo),
            sylph_python_dir: Some(root.join("repo/python")),
            virtual_env: Some(venv),
        };
        (root.join("bin/sylph"), root.join("lib/sylph/python"), env)
    }

    #[test]
    fn test_release_trusts_only_the_installed_directory() {
        let (exe, installed, env) = installed_layout("release");
        let dirs = python_dirs(BuildMode::Release, &exe, &env);
        assert_eq!(dirs, [installed]);
        let cwd = std::env::current_dir().unwrap();
        for dir in &dirs {
            assert!(
                !cwd.starts_with(dir),
                "{} is the cwd or an ancestor",
                dir.display()
            );
            assert!(!dir.starts_with(env!("CARGO_MANIFEST_DIR")));
            assert!(!dir.starts_with(env.repo.as_ref().unwrap()));
            assert!(!dir.starts_with(env.virtual_env.as_ref().unwrap()));
        }
    }

    #[test]
    fn test_release_without_an_install_trusts_nothing() {
        let root = tree("bare");
        assert!(python_dirs(
            BuildMode::Release,
            &root.join("sylph"),
            &PythonEnv::default()
        )
        .is_empty());
    }

    #[test]
    fn test_debug_puts_sylph_code_before_site_packages() {
        let (exe, installed, env) = installed_layout("debug");
        let dirs = python_dirs(BuildMode::Debug, &exe, &env);
        let repo = env.repo.clone().unwrap();
        assert_eq!(
            dirs,
            [
                repo.join("python"),
                installed,
                repo.join(".venv/lib/python3.13/site-packages"),
                env.virtual_env
                    .clone()
                    .unwrap()
                    .join("lib/python3.13/site-packages"),
            ]
        );
        // Never the launch cwd or its ancestors.
        let cwd = std::env::current_dir().unwrap();
        assert!(dirs.iter().all(|dir| !cwd.starts_with(dir)));
    }

    #[cfg(unix)]
    #[test]
    fn test_world_writable_directories_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let (exe, installed, env) = installed_layout("writable");
        let mode = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        mode(&installed, 0o777);
        assert!(python_dirs(BuildMode::Release, &exe, &env).is_empty());
        mode(&installed, 0o775);
        assert!(python_dirs(BuildMode::Release, &exe, &env).is_empty());
        // A writable parent is as bad: it can replace the directory.
        mode(&installed, 0o755);
        mode(installed.parent().unwrap(), 0o777);
        assert!(python_dirs(BuildMode::Release, &exe, &env).is_empty());
        mode(installed.parent().unwrap(), 0o755);
        assert_eq!(python_dirs(BuildMode::Release, &exe, &env), [installed]);
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_sys_path_is_set_up_once_and_modules_come_from_the_repo() {
        let path_len = || {
            Python::attach(|py| -> PyResult<usize> { py.import("sys")?.getattr("path")?.len() })
                .unwrap()
        };
        summarize_text("warm up the bridge");
        let before = path_len();
        summarize_text("one");
        export_rich_markdown("{}", &tmp("paths.md"));
        assert_eq!(path_len(), before, "sys.path must not grow per call");
        let file: String = with_python_module(EXPORT_MODULE, |module| {
            module.getattr("__file__")?.extract()
        })
        .unwrap();
        let expected = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../python/sylph_py")
            .canonicalize()
            .unwrap();
        assert!(
            Path::new(&file).starts_with(&expected),
            "{file} is not under {}",
            expected.display()
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_ping() {
        let result = ping();
        assert!(!result.is_empty());
        assert!(result.starts_with("Python "));
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_summarize_text() {
        let result =
            summarize_text("This is a test document with enough words to summarize properly.");
        assert!(!result.is_empty());
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_chat_with_doc() {
        let result = chat_with_doc("What is this about?", "This is a test document.");
        assert!(!result.is_empty());
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_export_to_docx() {
        let result = export_to_docx("# Test", &tmp("export.docx"));
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_export_to_pdf() {
        let result = export_to_pdf("# Test", &tmp("export.pdf"));
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_export_rich_pdf() {
        // Cover title and heading carry an em-dash and curly quotes, which
        // the old latin-1 core fonts could not draw.
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
        let result = export_rich_pdf(doc, &tmp("rich.pdf"));
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
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
        let result = export_rich_docx(doc, &tmp("rich.docx"));
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
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_rich_exports_use_the_documents_page_setup() {
        // Letter with narrow (36 pt) margins: python-docx's own template
        // is Letter with 1.25" sides and fpdf's default is A4 with 10 mm,
        // so both must come from the document instead.
        let doc = r#"{"page_size": "Letter", "landscape": false,
            "page_margins": {"top": 36.0, "bottom": 36.0, "left": 36.0, "right": 36.0},
            "blocks": [{"Paragraph": {"runs": [{"text": "x", "styles": []}],
                "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}]}"#;
        let r = export_rich_pdf(doc, &tmp("setup.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(&tmp("setup.pdf"), "/MediaBox [0 0 612.00 792.00]"));

        let r = export_rich_docx(doc, &tmp("setup.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let xml = docx_document_xml(&tmp("setup.docx"));
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
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_rich_exports_default_to_a4_and_honour_landscape() {
        // No page keys at all: Sylph's defaults (A4, 1-inch margins), not
        // the libraries' — the editor shows A4 for a new document.
        let body = r#""blocks": [{"Paragraph": {"runs": [{"text": "x", "styles": []}],
            "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}]"#;
        let portrait = format!("{{{body}}}");
        let r = export_rich_docx(&portrait, &tmp("a4.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let xml = docx_document_xml(&tmp("a4.docx"));
        assert!(xml.contains(r#"<w:pgSz w:w="11900" w:h="16840""#), "{xml}");
        assert!(xml.contains(r#"w:left="1440""#), "{xml}");
        let r = export_rich_pdf(&portrait, &tmp("a4.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(&tmp("a4.pdf"), "/MediaBox [0 0 595.00 842.00]"));

        let landscape = format!(r#"{{"page_size": "A4", "landscape": true, {body}}}"#);
        let r = export_rich_pdf(&landscape, &tmp("a4_landscape.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        assert!(pdf_has(
            &tmp("a4_landscape.pdf"),
            "/MediaBox [0 0 842.00 595.00]"
        ));
        let r = export_rich_docx(&landscape, &tmp("a4_landscape.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let xml = docx_document_xml(&tmp("a4_landscape.docx"));
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
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_rich_docx_uses_the_documents_typography() {
        // The canvas shows body_font at body_font_size and headings at
        // 28/22/18/16/14/12 pt; the template's Calibri 11 and theme
        // heading fonts must not replace them.
        let r = export_rich_docx(&typography_doc("Noto Serif"), &tmp("type.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let normal = docx_style(&tmp("type.docx"), "Normal");
        assert!(normal.contains(r#"w:ascii="Noto Serif""#), "{normal}");
        assert!(normal.contains(r#"<w:sz w:val="26"/>"#), "13 pt: {normal}");
        let h1 = docx_style(&tmp("type.docx"), "Heading1");
        assert!(h1.contains(r#"w:ascii="Noto Serif""#), "{h1}");
        // Word prefers theme fonts over w:ascii, so they must be gone.
        assert!(!h1.contains("w:asciiTheme"), "{h1}");
        assert!(h1.contains(r#"<w:sz w:val="56"/>"#), "28 pt: {h1}");
        assert!(
            h1.contains(r#"w:before="240" w:after="120""#),
            "12/6 pt: {h1}"
        );
    }

    /// Text drawn on the pages of a PDF, via poppler's `pdftotext -raw`
    /// (the fonts are embedded CID fonts, so the page streams hold glyph
    /// ids, not text). `None` when pdftotext is not installed.
    fn pdf_text(path: &str) -> Option<String> {
        let out = std::process::Command::new("pdftotext")
            .args(["-raw", path, "-"])
            .output()
            .ok()?;
        assert!(out.status.success(), "pdftotext failed on {path}");
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_docx_is_a_current_word_file_with_devanagari_fonts() {
        // The python-docx template declared Word 2010, so Word opened every
        // export in Compatibility Mode, and Devanagari had no complex-script
        // font, so it fell back to whatever the system picked.
        let nepali = "नेपालीमा काम गर्दै";
        let doc = format!(
            r#"{{"body_font": "EB Garamond", "blocks": [
                {{"Heading": {{"level": 2, "runs": [{{"text": "{nepali}", "styles": []}}]}}}},
                {{"Paragraph": {{"runs": [{{"text": "{nepali}", "styles": []}}],
                  "style": {{"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}}}}
            ]}}"#
        );
        for (path, serif) in [(tmp("nepali.docx"), true), (tmp("nepali_md.docx"), false)] {
            let r = if serif {
                export_rich_docx(&doc, &path)
            } else {
                export_to_docx(&format!("## {nepali}\n\n{nepali}"), &path)
            };
            assert!(r.starts_with("Exported"), "docx: {r}");
            let settings = docx_part(&path, "word/settings.xml");
            assert!(
                settings.contains(r#"w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15""#),
                "{settings}"
            );
            let styles = docx_part(&path, "word/styles.xml");
            let cs = if serif {
                r#"w:cs="Noto Serif Devanagari""#
            } else {
                r#"w:cs="Noto Sans Devanagari""#
            };
            // Normal and all six headings.
            assert!(styles.matches(cs).count() >= 7, "{styles}");
            assert!(styles.contains(r#"w:bidi="ne-NP""#), "{styles}");
            assert!(docx_part(&path, "word/document.xml").contains(nepali));
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_exports_follow_the_heading_styles() {
        // Heading 1 has its own font and spacing; Heading 2 keeps the
        // defaults (body font, automatic spacing).
        let doc = r#"{"body_font": "EB Garamond", "line_spacing": 1.15,
            "heading_styles": [
                {"line_spacing": 2.0, "font": "Hanken Grotesk", "size": 32.0,
                 "space_before": 20.0, "space_after": 3.0},
                {"line_spacing": null, "font": null}, {}, {}, {}, {}
            ],
            "blocks": [
                {"Heading": {"level": 1, "runs": [{"text": "Styled", "styles": []}]}},
                {"Heading": {"level": 2, "runs": [{"text": "Plain", "styles": []}]}},
                {"Paragraph": {"runs": [{"text": "Body", "styles": []}],
                  "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}
            ]}"#;
        let r = export_rich_docx(doc, &tmp("styles.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let styles = docx_part(&tmp("styles.docx"), "word/styles.xml");
        let style_xml = |id: &str| {
            let start = styles
                .find(&format!(r#"w:styleId="{id}""#))
                .expect("style present");
            let end = styles[start..].find("</w:style>").unwrap() + start;
            styles[start..end].to_string()
        };
        let h1 = style_xml("Heading1");
        assert!(h1.contains(r#"w:ascii="Hanken Grotesk""#), "{h1}");
        // Word writes a multiple of single spacing in 240ths, sizes in
        // half-points and paragraph spacing in twentieths of a point.
        assert!(h1.contains(r#"w:line="480""#), "{h1}");
        assert!(h1.contains(r#"<w:sz w:val="64"/>"#), "{h1}");
        assert!(h1.contains(r#"w:before="400""#), "{h1}");
        assert!(h1.contains(r#"w:after="60""#), "{h1}");
        let h2 = style_xml("Heading2");
        assert!(h2.contains(r#"w:ascii="EB Garamond""#), "{h2}");
        assert!(!h2.contains(r#"w:line="480""#), "{h2}");
        // Heading 2 keeps the default scale: 22 pt.
        assert!(h2.contains(r#"<w:sz w:val="44"/>"#), "{h2}");

        let r = export_rich_pdf(doc, &tmp("styles.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        if let Some(fonts) = pdf_fonts(&tmp("styles.pdf")) {
            assert!(fonts.contains("+HankenGroteskSemiBold "), "{fonts}");
            assert!(fonts.contains("+EBGaramondBold "), "{fonts}");
            assert!(fonts.contains("+EBGaramond "), "{fonts}");
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_every_bundled_body_font_is_embedded_as_itself() {
        for (font, embedded) in [
            ("Source Serif 4", "+SourceSerif4 "),
            ("Lora", "+Lora "),
            ("Inter", "+Inter "),
            ("Hanken Grotesk", "+HankenGrotesk "),
            ("JetBrains Mono", "+JetBrainsMono "),
        ] {
            let path = tmp(&format!("{}.pdf", font.replace(' ', "")));
            let r = export_rich_pdf(&typography_doc(font), &path);
            assert!(r.starts_with("Exported"), "{font}: {r}");
            let Some(fonts) = pdf_fonts(&path) else {
                return no_pdftotext();
            };
            assert!(fonts.contains(embedded), "{font}: {fonts}");
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_pdf_never_breaks_a_word_where_the_font_changes() {
        // Writing each run on its own let fpdf2 split a styled word that
        // started near the line end ("sam|e"). Shift an italic word along
        // the line so some copy lands on every position of a line end.
        let paragraphs: Vec<String> = (0..40)
            .map(|n| {
                format!(
                    r#"{{"Paragraph": {{"runs": [
                        {{"text": "{}lead ", "styles": []}},
                        {{"text": "Straddling", "styles": ["Italic"]}},
                        {{"text": " tail words follow here.", "styles": []}}],
                      "style": {{"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}}}}"#,
                    "word ".repeat(12) + &"i".repeat(n)
                )
            })
            .collect();
        let doc = format!(r#"{{"blocks": [{}]}}"#, paragraphs.join(","));
        let r = export_rich_pdf(&doc, &tmp("wrap_words.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let Some(text) = pdf_text(&tmp("wrap_words.pdf")) else {
            return no_pdftotext();
        };
        assert_eq!(
            text.matches("Straddling").count(),
            40,
            "a word was split: {text}"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_docx_headings_keep_their_style_bold_and_colour() {
        // Every plain run carried an explicit "not bold", which overrode
        // the Heading styles, and headings kept the template's theme blue.
        let r = export_rich_docx(&typography_doc("EB Garamond"), &tmp("bold.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let body = docx_part(&tmp("bold.docx"), "word/document.xml");
        assert!(!body.contains(r#"<w:b w:val="0"/>"#), "{body}");
        assert!(!body.contains(r#"<w:i w:val="0"/>"#), "{body}");
        let styles = docx_part(&tmp("bold.docx"), "word/styles.xml");
        let start = styles.find(r#"w:styleId="Heading1""#).unwrap();
        let h1 = &styles[start..start + styles[start..].find("</w:style>").unwrap()];
        assert!(h1.contains(r#"<w:color w:val="000000"/>"#), "{h1}");
        assert!(h1.contains("<w:b/>"), "{h1}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_character_formatting_reaches_both_exports() {
        // "Big" is 20 pt Lora on selected words; the link keeps its colour
        // and gets 14 pt.
        let doc = r#"{"body_font": "EB Garamond", "blocks": [
            {"Paragraph": {"runs": [
                {"text": "Normal ", "styles": []},
                {"text": "Big", "styles": [], "size": 20.0, "font": "Lora"},
                {"text": " link", "styles": [{"Link": "https://example.com"}], "size": 14.0}
            ], "style": {"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}
        ]}"#;
        let r = export_rich_docx(doc, &tmp("chars.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let body = docx_part(&tmp("chars.docx"), "word/document.xml");
        assert!(body.contains(r#"w:ascii="Lora""#), "{body}");
        assert!(body.contains(r#"<w:sz w:val="40"/>"#), "{body}");
        assert!(body.contains(r#"<w:sz w:val="28"/>"#), "link size: {body}");
        // Run properties in schema order inside the hyperlink run.
        let link = &body[body.find("<w:hyperlink").unwrap()..];
        let color = link.find("<w:color").unwrap();
        assert!(color < link.find("<w:sz").unwrap(), "{link}");
        assert!(
            link.find("<w:sz").unwrap() < link.find("<w:u ").unwrap(),
            "{link}"
        );

        let r = export_rich_pdf(doc, &tmp("chars.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        if let Some(fonts) = pdf_fonts(&tmp("chars.pdf")) {
            assert!(fonts.contains("+Lora "), "{fonts}");
            assert!(fonts.contains("+EBGaramond "), "{fonts}");
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_alignment_reaches_both_exports() {
        let long = "word ".repeat(60);
        let doc = format!(
            r#"{{"blocks": [
            {{"Heading": {{"level": 1, "runs": [{{"text": "Centred title", "styles": []}}], "alignment": "Center"}}}},
            {{"Paragraph": {{"runs": [{{"text": "{long}", "styles": []}}],
              "style": {{"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0, "alignment": "Justify"}}}}}},
            {{"Paragraph": {{"runs": [{{"text": "Right", "styles": []}}],
              "style": {{"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0, "alignment": "Right"}}}}}}
        ]}}"#
        );
        let r = export_rich_docx(&doc, &tmp("align.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let body = docx_part(&tmp("align.docx"), "word/document.xml");
        assert!(body.contains(r#"<w:jc w:val="center"/>"#), "{body}");
        assert!(body.contains(r#"<w:jc w:val="both"/>"#), "{body}");
        assert!(body.contains(r#"<w:jc w:val="right"/>"#), "{body}");
        let r = export_rich_pdf(&doc, &tmp("align.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
    }

    /// The fonts a PDF's pages use, via poppler's `pdffonts` (every
    /// registered font is written to the file, used or not).
    fn pdf_fonts(path: &str) -> Option<String> {
        let out = std::process::Command::new("pdffonts")
            .arg(path)
            .output()
            .ok()?;
        assert!(out.status.success(), "pdffonts failed on {path}");
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Skip note for checks that need poppler.
    fn no_pdftotext() {
        eprintln!("pdftotext not on PATH: text checks skipped");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
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
        let r = export_rich_pdf(&doc, &tmp("table_wrap.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let Some(text) = pdf_text(&tmp("table_wrap.pdf")) else {
            return no_pdftotext();
        };
        assert!(text.contains("Header"), "header row drawn");
        assert!(text.contains("TAILWORD"), "rich table cell cut short");

        let markdown = format!("| H |\n|---|\n| {long} |\n");
        let r = export_to_pdf(&markdown, &tmp("table_wrap_md.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let text = pdf_text(&tmp("table_wrap_md.pdf")).expect("pdftotext");
        assert!(text.contains("TAILWORD"), "markdown table cell cut short");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_rich_pdf_embeds_the_bundled_family_for_the_typeface() {
        // Serif faces map to EB Garamond, sans to Hanken Grotesk, and every
        // font is embedded: no Base-14 (latin-1 only) font is left.
        let r = export_rich_pdf(&typography_doc("Noto Serif"), &tmp("serif.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let r = export_rich_pdf(&typography_doc("Noto Sans"), &tmp("sans.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        for pdf in ["serif.pdf", "sans.pdf"] {
            assert!(pdf_has(&tmp(pdf), "/FontFile2"), "{pdf}: fonts embedded");
            assert!(
                !pdf_has(&tmp(pdf), "/Subtype /Type1"),
                "{pdf}: a Base-14 font"
            );
        }
        let Some(serif) = pdf_fonts(&tmp("serif.pdf")) else {
            return no_pdftotext();
        };
        assert!(serif.contains("+EBGaramond "), "{serif}");
        assert!(serif.contains("+EBGaramondBold "), "{serif}");
        let sans = pdf_fonts(&tmp("sans.pdf")).unwrap();
        assert!(sans.contains("+HankenGroteskSemiBold "), "{sans}");
        assert!(!sans.contains("+EBGaramond"), "{sans}");
        // Every used font embedded, subset and with a Unicode map (the
        // emb, sub and uni columns of pdffonts).
        for line in serif.lines().chain(sans.lines()) {
            if line.contains('+') {
                assert!(line.contains("yes yes yes"), "{line}");
            }
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_nepali_survives_pdf_export() {
        let nepali = "क्ष त्र ज्ञ श्रृ ह्र — नेपाल → ≤ ≥";
        let doc = format!(
            r#"{{"blocks": [{{"Paragraph": {{"runs": [{{"text": "{nepali}", "styles": []}}],
                "style": {{"line_spacing": 1.15, "space_before": 0.0, "space_after": 8.0}}}}}}]}}"#
        );
        let r = export_rich_pdf(&doc, &tmp("nepali.pdf"));
        // Every character is covered, so there is no warning.
        assert_eq!(r, format!("Exported to {}", tmp("nepali.pdf")));
        if let Some(fonts) = pdf_fonts(&tmp("nepali.pdf")) {
            assert!(fonts.contains("+NotoSerifDevanagari "), "{fonts}");
        }
        assert!(!pdf_has(&tmp("nepali.pdf"), "/Subtype /Type1"));
        let Some(text) = pdf_text(&tmp("nepali.pdf")) else {
            return no_pdftotext();
        };
        // pdftotext spaces text by position, so compare without spaces.
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        assert!(squash(&text).contains(&squash(nepali)), "got: {text}");
        assert!(
            !text.contains('?'),
            "a covered character became '?': {text}"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_pdf_export_warns_about_characters_no_font_has() {
        let r = export_to_pdf("Smile 😀", &tmp("emoji.pdf"));
        assert!(r.starts_with("Exported to "), "pdf: {r}");
        assert!(
            r.contains("Warning: no bundled font has 😀 (U+1F600)"),
            "{r}"
        );
        // The warning belongs to that export only.
        let r = export_to_pdf("Plain", &tmp("plain.pdf"));
        assert!(!r.contains("Warning"), "{r}");
    }

    #[test]
    fn test_fonts_dir_is_trusted_like_the_code() {
        let (exe, _, env) = installed_layout("fonts");
        let root = exe.parent().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(fonts_dir(BuildMode::Release, &exe, &env), None);
        std::fs::create_dir_all(root.join("share/sylph/fonts")).unwrap();
        assert_eq!(
            fonts_dir(BuildMode::Release, &exe, &env),
            Some(root.join("share/sylph/fonts"))
        );
        // Debug prefers the repo's assets; release never looks there.
        std::fs::create_dir_all(root.join("repo/assets/fonts")).unwrap();
        assert_eq!(
            fonts_dir(BuildMode::Debug, &exe, &env),
            Some(root.join("repo/assets/fonts"))
        );
        assert_eq!(
            fonts_dir(BuildMode::Release, &exe, &env),
            Some(root.join("share/sylph/fonts"))
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_export_rich_markdown() {
        let doc = r#"{"blocks": [
            {"Heading": {"level": 1, "runs": [{"text": "T", "styles": []}]}}
        ]}"#;
        let result = export_rich_markdown(doc, &tmp("rich.md"));
        assert!(result.starts_with("Exported"), "got: {result}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
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

        let r = export_rich_docx(doc, &tmp("kitchen.docx"));
        assert!(r.starts_with("Exported"), "docx: {r}");
        let r = export_rich_pdf(doc, &tmp("kitchen.pdf"));
        assert!(r.starts_with("Exported"), "pdf: {r}");
        let r = export_rich_markdown(doc, &tmp("kitchen.md"));
        assert!(r.starts_with("Exported"), "md: {r}");

        let md = std::fs::read_to_string(tmp("kitchen.md")).unwrap();
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
    #[cfg_attr(
        not(feature = "python-tests"),
        ignore = "needs the Python venv: --features python-tests"
    )]
    fn test_api_signatures_compile() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<String>();
        assert_sync::<String>();

        // Verify all public functions exist and have correct signatures
        let _: String = ping();
        let _: String = summarize_text("test");
        let _: String = chat_with_doc("q", "ctx");
        let _: String = export_to_docx("md", &tmp("sig.docx"));
        let _: String = export_to_pdf("md", &tmp("sig.pdf"));
    }
}
