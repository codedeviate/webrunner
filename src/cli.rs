use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "webrunner", version, about = "Development web server with CGI support")]
pub struct CliConfig {
    /// HTTP port
    #[arg(short = 'p', long, default_value_t = 8080)]
    pub port: u16,

    /// Enable HTTPS
    #[arg(long)]
    pub https: bool,

    /// HTTPS port
    #[arg(long, default_value_t = 8443)]
    pub https_port: u16,

    /// Path to TLS certificate (PEM)
    #[arg(long)]
    pub cert: Option<String>,

    /// Path to TLS private key (PEM)
    #[arg(long)]
    pub key: Option<String>,

    /// Disable directory listing
    #[arg(long)]
    pub no_index: bool,
}

impl CliConfig {
    /// Returns true if HTTPS should be active (either --https flag or --cert+--key supplied)
    pub fn https_active(&self) -> bool {
        self.https || (self.cert.is_some() && self.key.is_some())
    }

    /// Validate that --cert and --key are either both present or both absent.
    /// Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        match (self.cert.as_ref(), self.key.as_ref()) {
            (Some(_), None) => Err("--cert requires --key".to_string()),
            (None, Some(_)) => Err("--key requires --cert".to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_defaults() {
        let cfg = CliConfig::parse_from(["webrunner"]);
        assert_eq!(cfg.port, 8080);
        assert!(!cfg.https);
        assert_eq!(cfg.https_port, 8443);
        assert!(cfg.cert.is_none());
        assert!(cfg.key.is_none());
        assert!(!cfg.no_index);
    }

    #[test]
    fn test_custom_port() {
        let cfg = CliConfig::parse_from(["webrunner", "--port", "9000"]);
        assert_eq!(cfg.port, 9000);
    }

    #[test]
    fn test_https_flag() {
        let cfg = CliConfig::parse_from(["webrunner", "--https"]);
        assert!(cfg.https);
    }

    #[test]
    fn test_cert_and_key() {
        let cfg = CliConfig::parse_from(["webrunner", "--cert", "/path/cert.pem", "--key", "/path/key.pem"]);
        assert_eq!(cfg.cert.as_deref(), Some("/path/cert.pem"));
        assert_eq!(cfg.key.as_deref(), Some("/path/key.pem"));
    }

    #[test]
    fn test_https_enabled_implies_https() {
        let cfg = CliConfig::parse_from(["webrunner", "--cert", "/c.pem", "--key", "/k.pem"]);
        assert!(cfg.https_active());
    }

    #[test]
    fn test_validate_cert_without_key() {
        let cfg = CliConfig::parse_from(["webrunner", "--cert", "/c.pem"]);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_key_without_cert() {
        let cfg = CliConfig::parse_from(["webrunner", "--key", "/k.pem"]);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_both_ok() {
        let cfg = CliConfig::parse_from(["webrunner", "--cert", "/c.pem", "--key", "/k.pem"]);
        assert!(cfg.validate().is_ok());
    }
}
