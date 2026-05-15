use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lower")]
pub enum Compression {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Gzip,
    Br,
}

impl Encoding {
    pub fn header_value(self) -> &'static str {
        match self {
            Encoding::Gzip => "gzip",
            Encoding::Br => "br",
        }
    }

    /// ETag suffix per RFC 7232 §2.3.3 (Apache convention).
    pub fn etag_suffix(self) -> &'static str {
        match self {
            Encoding::Gzip => "-gzip",
            Encoding::Br => "-br",
        }
    }
}

/// Pick the preferred encoding from an `Accept-Encoding` header value.
/// Returns `None` if the client offers neither `br` nor `gzip`.
/// Prefers brotli when both are offered. Ignores `q=` weighting.
pub fn pick_encoding(accept_encoding: &str) -> Option<Encoding> {
    let lower = accept_encoding.to_ascii_lowercase();
    let offers_br = lower.split(',').any(|tok| tok.trim().starts_with("br"));
    let offers_gz = lower.split(',').any(|tok| {
        let t = tok.trim();
        t.starts_with("gzip") || t.starts_with("x-gzip")
    });
    if offers_br {
        Some(Encoding::Br)
    } else if offers_gz {
        Some(Encoding::Gzip)
    } else {
        None
    }
}

/// True if the MIME type is in webrunner's compress allow-list.
pub fn is_compressible(mime: &str) -> bool {
    // Strip parameters like `; charset=utf-8`.
    let base = mime.split(';').next().unwrap_or("").trim();
    base.starts_with("text/")
        || matches!(
            base,
            "application/json"
                | "application/javascript"
                | "application/xml"
                | "image/svg+xml"
                | "application/wasm"
        )
}

/// Compress `data` with the chosen encoding. Returns the compressed bytes.
#[allow(dead_code)] // wired in Task 3
pub fn compress(encoding: Encoding, data: &[u8]) -> Vec<u8> {
    match encoding {
        Encoding::Gzip => {
            use flate2::write::GzEncoder;
            use flate2::Compression as GzLevel;
            let mut enc = GzEncoder::new(Vec::with_capacity(data.len() / 2), GzLevel::default());
            enc.write_all(data).expect("gzip write to Vec");
            enc.finish().expect("gzip finish")
        }
        Encoding::Br => {
            let mut out = Vec::with_capacity(data.len() / 2);
            let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 4, 22);
            writer.write_all(data).expect("brotli write to Vec");
            drop(writer);
            out
        }
    }
}

/// Minimum payload size for compression to be worthwhile.
#[allow(dead_code)] // wired in Task 3
pub const MIN_COMPRESS_SIZE: u64 = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_encoding_prefers_brotli() {
        assert_eq!(pick_encoding("gzip, br"), Some(Encoding::Br));
        assert_eq!(pick_encoding("br"), Some(Encoding::Br));
        assert_eq!(pick_encoding("GZIP, BR"), Some(Encoding::Br));
    }

    #[test]
    fn test_pick_encoding_gzip_only() {
        assert_eq!(pick_encoding("gzip"), Some(Encoding::Gzip));
        assert_eq!(pick_encoding("x-gzip"), Some(Encoding::Gzip));
    }

    #[test]
    fn test_pick_encoding_none() {
        assert_eq!(pick_encoding(""), None);
        assert_eq!(pick_encoding("deflate, zstd"), None);
    }

    #[test]
    fn test_is_compressible_allow_list() {
        assert!(is_compressible("text/html"));
        assert!(is_compressible("text/css; charset=utf-8"));
        assert!(is_compressible("application/json"));
        assert!(is_compressible("image/svg+xml"));
        assert!(is_compressible("application/wasm"));
        assert!(!is_compressible("image/png"));
        assert!(!is_compressible("video/mp4"));
        assert!(!is_compressible("application/octet-stream"));
        assert!(!is_compressible("audio/wav"));
    }

    #[test]
    fn test_compress_gzip_round_trip() {
        use std::io::Read;
        let payload = "hello world ".repeat(100); // ~1.2 KB
        let compressed = compress(Encoding::Gzip, payload.as_bytes());
        let mut decoder = flate2::read::GzDecoder::new(compressed.as_slice());
        let mut out = String::new();
        decoder.read_to_string(&mut out).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn test_compress_brotli_round_trip() {
        use std::io::Read;
        let payload = "hello world ".repeat(100); // ~1.2 KB
        let compressed = compress(Encoding::Br, payload.as_bytes());
        let mut decoder = brotli::Decompressor::new(compressed.as_slice(), 4096);
        let mut out = String::new();
        decoder.read_to_string(&mut out).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn test_etag_suffix_values() {
        assert_eq!(Encoding::Gzip.etag_suffix(), "-gzip");
        assert_eq!(Encoding::Br.etag_suffix(), "-br");
        assert_eq!(Encoding::Gzip.header_value(), "gzip");
        assert_eq!(Encoding::Br.header_value(), "br");
    }
}
