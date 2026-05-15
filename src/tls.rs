use std::path::{Path, PathBuf};

/// Paths to the cert and key that should be used for HTTPS.
pub struct CertPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// Default config directory for auto-generated certs.
pub fn default_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("webrunner")
}

/// Generate a self-signed certificate and private key, writing PEM files to `cert_path` and `key_path`.
pub fn generate_self_signed(cert_path: &Path, key_path: &Path) -> Result<(), String> {
    use rcgen::{generate_simple_self_signed, CertifiedKey};

    let subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];

    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(subject_alt_names)
        .map_err(|e| format!("Failed to generate certificate: {}", e))?;

    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir {:?}: {}", parent, e))?;
    }

    std::fs::write(cert_path, cert.pem())
        .map_err(|e| format!("Failed to write cert to {:?}: {}", cert_path, e))?;

    std::fs::write(key_path, key_pair.serialize_pem())
        .map_err(|e| format!("Failed to write key to {:?}: {}", key_path, e))?;

    Ok(())
}

/// Resolve cert and key paths, generating a self-signed cert if needed.
/// - If `cert` and `key` are both Some: use them (user-supplied).
/// - If both None: use ~/.config/webrunner/cert.pem and key.pem, generating if absent.
pub fn ensure_cert(cert: Option<&str>, key: Option<&str>) -> Result<CertPaths, String> {
    match (cert, key) {
        (Some(c), Some(k)) => {
            let cert_path = PathBuf::from(c);
            let key_path = PathBuf::from(k);
            if !cert_path.exists() {
                return Err(format!("Certificate file not found: {:?}", cert_path));
            }
            if !key_path.exists() {
                return Err(format!("Key file not found: {:?}", key_path));
            }
            Ok(CertPaths { cert: cert_path, key: key_path })
        }
        (None, None) => {
            let config_dir = default_config_dir();
            let cert_path = config_dir.join("cert.pem");
            let key_path = config_dir.join("key.pem");

            if !cert_path.exists() || !key_path.exists() {
                log::info!("Generating self-signed certificate in {:?}", config_dir);
                generate_self_signed(&cert_path, &key_path)?;
            }

            Ok(CertPaths { cert: cert_path, key: key_path })
        }
        _ => Err("--cert and --key must be provided together".to_string()),
    }
}

/// Compute a simple fingerprint of a PEM certificate file (for display purposes).
pub fn cert_fingerprint(cert_path: &Path) -> String {
    let pem = match std::fs::read(cert_path) {
        Ok(b) => b,
        Err(_) => return "(unknown)".to_string(),
    };
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    pem.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_cert_creates_files() {
        let tmp = TempDir::new().unwrap();
        let cert_path = tmp.path().join("cert.pem");
        let key_path = tmp.path().join("key.pem");

        generate_self_signed(&cert_path, &key_path).unwrap();

        assert!(cert_path.exists());
        assert!(key_path.exists());
        let cert_content = std::fs::read_to_string(&cert_path).unwrap();
        assert!(cert_content.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_ensure_cert_reuses_existing() {
        let tmp = TempDir::new().unwrap();
        let cert_path = tmp.path().join("cert.pem");
        let key_path = tmp.path().join("key.pem");

        generate_self_signed(&cert_path, &key_path).unwrap();
        let content_before = std::fs::read_to_string(&cert_path).unwrap();

        ensure_cert(Some(cert_path.to_str().unwrap()), Some(key_path.to_str().unwrap())).unwrap();
        let content_after = std::fs::read_to_string(&cert_path).unwrap();

        assert_eq!(content_before, content_after);
    }

    #[test]
    fn test_ensure_cert_missing_file_errors() {
        let result = ensure_cert(Some("/nonexistent/cert.pem"), Some("/nonexistent/key.pem"));
        assert!(result.is_err());
    }
}
