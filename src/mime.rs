// MIME type detection

/// Returns the MIME type for a file extension.
/// `extra` is a list of AddType overrides from .htaccess (extension, mime_type).
/// Falls back to "application/octet-stream" for unknown extensions.
pub fn mime_for_ext(ext: &str, _extra: &[(String, String)]) -> &'static str {
    // Static built-in map
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm"  => "text/html; charset=utf-8",
        "css"           => "text/css",
        "js" | "mjs"    => "application/javascript",
        "json"          => "application/json",
        "xml"           => "application/xml",
        "txt"           => "text/plain; charset=utf-8",
        "md"            => "text/markdown",
        "png"           => "image/png",
        "jpg" | "jpeg"  => "image/jpeg",
        "gif"           => "image/gif",
        "svg"           => "image/svg+xml",
        "ico"           => "image/x-icon",
        "webp"          => "image/webp",
        "avif"          => "image/avif",
        "mp4"           => "video/mp4",
        "webm"          => "video/webm",
        "mp3"           => "audio/mpeg",
        "ogg"           => "audio/ogg",
        "wav"           => "audio/wav",
        "pdf"           => "application/pdf",
        "zip"           => "application/zip",
        "wasm"          => "application/wasm",
        "ttf"           => "font/ttf",
        "woff"          => "font/woff",
        "woff2"         => "font/woff2",
        "otf"           => "font/otf",
        _               => "application/octet-stream",
    }
}

/// Returns the MIME type as an owned String, respecting AddType overrides.
pub fn mime_for_ext_owned(
    ext: &str,
    extra: &[crate::htaccess::AddTypeEntry],
    filename: &str,
) -> String {
    let ext_lower = ext.to_ascii_lowercase();
    // Walk entries in order; later entries override earlier ones (Apache
    // merge order). An entry applies when its `ext` matches AND its
    // `file_scope` matches the filename.
    let mut chosen: Option<&str> = None;
    for entry in extra {
        if !entry.ext.eq_ignore_ascii_case(&ext_lower) {
            continue;
        }
        if !entry.file_scope.iter().all(|re| re.is_match(filename)) {
            continue;
        }
        chosen = Some(entry.mime.as_str());
    }
    if let Some(m) = chosen {
        return m.to_string();
    }
    mime_for_ext(&ext_lower, &[]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html() {
        assert_eq!(mime_for_ext("html", &[]), "text/html; charset=utf-8");
    }

    #[test]
    fn test_js() {
        assert_eq!(mime_for_ext("js", &[]), "application/javascript");
    }

    #[test]
    fn test_unknown_defaults_to_octet_stream() {
        assert_eq!(mime_for_ext("xyz123", &[]), "application/octet-stream");
    }

    #[test]
    fn test_addtype_override() {
        use crate::htaccess::AddTypeEntry;
        let extra = vec![AddTypeEntry {
            file_scope: Vec::new(),
            ext: "xyz".to_string(),
            mime: "application/x-custom".to_string(),
        }];
        assert_eq!(
            mime_for_ext_owned("xyz", &extra, "file.xyz"),
            "application/x-custom"
        );
    }

    #[test]
    fn test_addtype_does_not_override_unrelated() {
        use crate::htaccess::AddTypeEntry;
        let extra = vec![AddTypeEntry {
            file_scope: Vec::new(),
            ext: "xyz".to_string(),
            mime: "application/x-custom".to_string(),
        }];
        assert_eq!(
            mime_for_ext_owned("html", &extra, "page.html"),
            "text/html; charset=utf-8"
        );
    }

    #[test]
    fn addtype_scoped_only_applies_to_matching_files() {
        use crate::htaccess::AddTypeEntry;
        use regex::Regex;
        let scope = vec![Regex::new("^.*\\.special\\.xls$").unwrap()];
        let entries = vec![AddTypeEntry {
            file_scope: scope,
            ext: "xls".to_string(),
            mime: "application/x-special".to_string(),
        }];
        // Scoped: only applies when filename matches the scope.
        assert_eq!(
            mime_for_ext_owned("xls", &entries, "data.special.xls"),
            "application/x-special"
        );
        // Same extension, non-matching filename → falls back to built-in.
        // The built-in for xls is "application/octet-stream" (no special case).
        assert_eq!(
            mime_for_ext_owned("xls", &entries, "data.xls"),
            "application/octet-stream"
        );
    }
}
