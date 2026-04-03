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
pub fn mime_for_ext_owned(ext: &str, extra: &[(String, String)]) -> String {
    let ext_lower = ext.to_ascii_lowercase();
    for (e, m) in extra {
        if e.eq_ignore_ascii_case(&ext_lower) {
            return m.clone();
        }
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
        let extra = vec![("xyz".to_string(), "application/x-custom".to_string())];
        assert_eq!(mime_for_ext_owned("xyz", &extra), "application/x-custom");
    }

    #[test]
    fn test_addtype_does_not_override_unrelated() {
        let extra = vec![("xyz".to_string(), "application/x-custom".to_string())];
        assert_eq!(mime_for_ext_owned("html", &extra), "text/html; charset=utf-8");
    }
}
