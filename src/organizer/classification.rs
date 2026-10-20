use super::model::Category;
use std::path::Path;

pub(super) fn extension_for_name(file_name: &str) -> Option<String> {
    let (_, extension) = file_name.rsplit_once('.')?;

    // A single leading dot introduces a hidden filename, not an extension.
    if extension.is_empty() || (file_name.starts_with('.') && !file_name[1..].contains('.')) {
        return None;
    }

    Some(extension.to_lowercase())
}

pub(super) fn classify_file_name(file_name: &str) -> (Option<String>, Category) {
    let extension = extension_for_name(file_name);
    let category = match extension.as_deref() {
        Some("pdf" | "doc" | "docx" | "txt" | "rtf" | "odt") => Category::Documents,
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "heic") => Category::Images,
        Some("mp4" | "mov" | "mkv" | "avi" | "webm") => Category::Videos,
        Some("mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg") => Category::Audio,
        Some("zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar") => Category::Archives,
        Some(
            "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "css"
            | "html" | "json" | "yaml" | "yml" | "toml" | "sh",
        ) => Category::Code,
        Some("csv" | "xls" | "xlsx" | "ods") => Category::Spreadsheets,
        Some("ppt" | "pptx" | "odp") => Category::Presentations,
        _ => Category::Other,
    };
    (extension, category)
}

pub(super) fn is_hidden_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}
