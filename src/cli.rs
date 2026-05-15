use clap::Parser;
use std::net::IpAddr;
use std::str::FromStr;

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

    /// Additional file extensions to execute as CGI (e.g. --cgi js,ts).
    /// Always-on CGI extensions (pl, php) are independent of this flag.
    /// Accepted values: js, ts, pl, php. Case-insensitive. Comma-separated
    /// and/or repeated.
    #[arg(long, value_delimiter = ',')]
    pub cgi: Vec<String>,

    /// Bind addresses (IPv4 or IPv6 literals). Comma-separated and/or
    /// repeatable. Default: 0.0.0.0,::
    #[arg(long, value_delimiter = ',')]
    pub bind: Vec<String>,

    /// True iff the user explicitly passed --bind. Drives fail-hard vs.
    /// fail-soft semantics in server::run. Derived in validate(); not
    /// parsed from CLI.
    #[clap(skip)]
    pub bind_explicit: bool,

    /// Directory to serve. Default: current working directory.
    /// Mutually exclusive with the positional <DIR> argument.
    #[arg(long, value_name = "DIR")]
    pub root: Option<String>,

    /// Directory to serve (positional). Same semantics as --root.
    #[arg(value_name = "DIR")]
    pub root_pos: Option<String>,

    /// Print usage examples and exit
    #[arg(long)]
    pub examples: bool,
}

impl CliConfig {
    /// Returns true if HTTPS should be active (either --https flag or --cert+--key supplied)
    pub fn https_active(&self) -> bool {
        self.https || (self.cert.is_some() && self.key.is_some())
    }

    /// Validate CLI arguments. Normalises `cgi` and `bind` in place,
    /// fills the default bind set, records `bind_explicit`, and rejects
    /// passing both `--root` and the positional <DIR> together.
    /// Returns an error message if invalid.
    pub fn validate(&mut self) -> Result<(), String> {
        match (self.cert.as_ref(), self.key.as_ref()) {
            (Some(_), None) => return Err("--cert requires --key".to_string()),
            (None, Some(_)) => return Err("--key requires --cert".to_string()),
            _ => {}
        }

        const ALLOWED_CGI: &[&str] = &["js", "ts", "pl", "php"];
        let mut normalised: Vec<String> = Vec::new();
        for raw in &self.cgi {
            let lower = raw.to_lowercase();
            if !ALLOWED_CGI.contains(&lower.as_str()) {
                return Err(format!(
                    "--cgi: unknown extension '{}' (allowed: js, ts, pl, php)",
                    raw
                ));
            }
            if !normalised.contains(&lower) {
                normalised.push(lower);
            }
        }
        self.cgi = normalised;

        // Must check before filling the default so bind_explicit reflects user input.
        self.bind_explicit = !self.bind.is_empty();
        if self.bind.is_empty() {
            self.bind = vec!["0.0.0.0".to_string(), "::".to_string()];
        }
        let mut canonical: Vec<String> = Vec::new();
        for raw in &self.bind {
            let ip = IpAddr::from_str(raw).map_err(|_| {
                format!(
                    "--bind: invalid IP literal '{}' (hostnames and ports not allowed here)",
                    raw
                )
            })?;
            let canon = ip.to_string();
            if !canonical.contains(&canon) {
                canonical.push(canon);
            }
        }
        self.bind = canonical;

        if self.root.is_some() && self.root_pos.is_some() {
            return Err(
                "--root and the positional <DIR> argument are mutually exclusive"
                    .to_string(),
            );
        }

        Ok(())
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
        let mut cfg = CliConfig::parse_from(["webrunner", "--cert", "/c.pem"]);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_key_without_cert() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--key", "/k.pem"]);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_both_ok() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--cert", "/c.pem", "--key", "/k.pem"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_cgi_default_empty() {
        let cfg = CliConfig::parse_from(["webrunner"]);
        assert!(cfg.cgi.is_empty());
    }

    #[test]
    fn test_cgi_comma_list() {
        let cfg = CliConfig::parse_from(["webrunner", "--cgi", "js,ts"]);
        assert_eq!(cfg.cgi, vec!["js".to_string(), "ts".to_string()]);
    }

    #[test]
    fn test_cgi_repeated_flag() {
        let cfg = CliConfig::parse_from(["webrunner", "--cgi", "js", "--cgi", "ts"]);
        assert_eq!(cfg.cgi, vec!["js".to_string(), "ts".to_string()]);
    }

    #[test]
    fn test_cgi_validate_lowercases_and_dedups() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--cgi", "JS,js,Ts"]);
        cfg.validate().unwrap();
        assert_eq!(cfg.cgi, vec!["js".to_string(), "ts".to_string()]);
    }

    #[test]
    fn test_cgi_validate_accepts_pl_php_as_noop() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--cgi", "pl,php"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_cgi_validate_rejects_unknown() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--cgi", "html"]);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("--cgi"));
        assert!(err.contains("html"));
    }

    #[test]
    fn test_bind_default_empty_before_validate() {
        let cfg = CliConfig::parse_from(["webrunner"]);
        assert!(cfg.bind.is_empty());
        assert!(!cfg.bind_explicit);
    }

    #[test]
    fn test_bind_comma_list_parses() {
        let cfg = CliConfig::parse_from(["webrunner", "--bind", "127.0.0.1,::1"]);
        assert_eq!(cfg.bind, vec!["127.0.0.1".to_string(), "::1".to_string()]);
    }

    #[test]
    fn test_bind_repeated_flag_parses() {
        let cfg = CliConfig::parse_from(["webrunner", "--bind", "127.0.0.1", "--bind", "::1"]);
        assert_eq!(cfg.bind, vec!["127.0.0.1".to_string(), "::1".to_string()]);
    }

    #[test]
    fn test_bind_validate_fills_default() {
        let mut cfg = CliConfig::parse_from(["webrunner"]);
        cfg.validate().unwrap();
        assert_eq!(cfg.bind, vec!["0.0.0.0".to_string(), "::".to_string()]);
        assert!(!cfg.bind_explicit);
    }

    #[test]
    fn test_bind_validate_marks_explicit() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "127.0.0.1"]);
        cfg.validate().unwrap();
        assert_eq!(cfg.bind, vec!["127.0.0.1".to_string()]);
        assert!(cfg.bind_explicit);
    }

    #[test]
    fn test_bind_validate_canonicalises_ipv6() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "0:0:0:0:0:0:0:1"]);
        cfg.validate().unwrap();
        assert_eq!(cfg.bind, vec!["::1".to_string()]);
    }

    #[test]
    fn test_bind_validate_dedupes() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "127.0.0.1,127.0.0.1"]);
        cfg.validate().unwrap();
        assert_eq!(cfg.bind, vec!["127.0.0.1".to_string()]);
    }

    #[test]
    fn test_bind_validate_rejects_hostname() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "localhost"]);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("--bind"));
        assert!(err.contains("localhost"));
    }

    #[test]
    fn test_bind_validate_rejects_port_suffix() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "127.0.0.1:8080"]);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("--bind"));
        assert!(err.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_bind_validate_rejects_garbage() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "not-an-ip"]);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("--bind"));
        assert!(err.contains("not-an-ip"));
    }

    #[test]
    fn test_bind_validate_accepts_ipv4_and_ipv6_mixed() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--bind", "0.0.0.0,::,127.0.0.1,::1"]);
        cfg.validate().unwrap();
        assert_eq!(cfg.bind, vec![
            "0.0.0.0".to_string(),
            "::".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ]);
        assert!(cfg.bind_explicit);
    }

    #[test]
    fn test_root_flag_parses() {
        let cfg = CliConfig::parse_from(["webrunner", "--root", "./docs"]);
        assert_eq!(cfg.root.as_deref(), Some("./docs"));
        assert!(cfg.root_pos.is_none());
    }

    #[test]
    fn test_root_positional_parses() {
        let cfg = CliConfig::parse_from(["webrunner", "./docs"]);
        assert!(cfg.root.is_none());
        assert_eq!(cfg.root_pos.as_deref(), Some("./docs"));
    }

    #[test]
    fn test_root_default_both_none() {
        let cfg = CliConfig::parse_from(["webrunner"]);
        assert!(cfg.root.is_none());
        assert!(cfg.root_pos.is_none());
    }

    #[test]
    fn test_root_validate_rejects_both_forms() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--root", "./a", "./b"]);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("--root"));
        assert!(err.contains("positional"));
    }

    #[test]
    fn test_root_validate_accepts_flag_alone() {
        let mut cfg = CliConfig::parse_from(["webrunner", "--root", "./a"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_root_validate_accepts_positional_alone() {
        let mut cfg = CliConfig::parse_from(["webrunner", "./a"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_root_validate_accepts_neither() {
        let mut cfg = CliConfig::parse_from(["webrunner"]);
        assert!(cfg.validate().is_ok());
    }
}
