//! One export: the snapshot taken on the UI thread and the blocking
//! render that runs on a background thread.

use crate::ExportFormat;
use std::path::Path;

/// A document snapshot for `run_export`.
pub(crate) struct ExportJob {
    pub(crate) json: Result<String, String>,
    pub(crate) content: String,
}

/// Write `job` to `path` through the Python renderers. Blocking; runs on a
/// background thread. Returns the bridge's message.
pub(crate) fn run_export(format: ExportFormat, job: ExportJob, path: &Path) -> String {
    let out = path.to_string_lossy().into_owned();
    match job.json {
        Ok(json) => match format {
            ExportFormat::Pdf => sylph_py_bridge::export_rich_pdf(&json, &out),
            ExportFormat::Docx => sylph_py_bridge::export_rich_docx(&json, &out),
            ExportFormat::Markdown => sylph_py_bridge::export_rich_markdown(&json, &out),
        },
        Err(e) => match format {
            ExportFormat::Pdf => {
                // Never write DOCX bytes into a .pdf path.
                format!("Export failed to serialize document: {e}")
            }
            ExportFormat::Docx => {
                let fallback = sylph_py_bridge::export_to_docx(&job.content, &out);
                format!("Export failed to serialize document: {e}. Fallback: {fallback}")
            }
            ExportFormat::Markdown => format!("Export failed to serialize document: {e}"),
        },
    }
}

pub(crate) fn file_name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The status-bar line for a finished export: where it went, or why not.
/// A warning the exporter attached ("Exported to …. Warning: …") stays.
pub(crate) fn export_message(path: &Path, result: String) -> String {
    let Some(done) = result.strip_prefix("Exported to ") else {
        return result;
    };
    let folder = path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let message = format!("Exported · {} (in {folder})", file_name_of(path));
    match done.split_once(". Warning: ") {
        Some((_, warning)) => format!("{message}. Warning: {warning}"),
        None => message,
    }
}

#[cfg(test)]
mod export_message_tests {
    use super::{export_message, file_name_of};
    use std::path::Path;

    #[test]
    fn a_finished_export_names_the_file_and_folder() {
        let path = Path::new("/home/u/Documents/Report.pdf");
        assert_eq!(
            export_message(path, "Exported to /home/u/Documents/Report.pdf".into()),
            "Exported · Report.pdf (in /home/u/Documents)"
        );
        assert_eq!(
            export_message(path, "Python error: boom".into()),
            "Python error: boom"
        );
        assert_eq!(
            export_message(
                path,
                "Exported to /home/u/Documents/Report.pdf. Warning: no bundled font has 😀".into()
            ),
            "Exported · Report.pdf (in /home/u/Documents). Warning: no bundled font has 😀"
        );
        assert_eq!(file_name_of(Path::new("/")), "/");
    }
}
