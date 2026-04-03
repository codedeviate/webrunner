// Basic auth and .htpasswd verification

use base64::{Engine, engine::general_purpose::STANDARD};
use sha1::{Sha1, Digest};

pub fn verify_htpasswd(password: &str, hash: &str) -> bool {
    if hash.starts_with("$2y$") || hash.starts_with("$2a$") || hash.starts_with("$2b$") {
        bcrypt::verify(password, hash).unwrap_or(false)
    } else if hash.starts_with("{SHA}") {
        verify_sha1(password, hash)
    } else if hash.starts_with("$apr1$") {
        verify_apr1(password, hash)
    } else {
        // plaintext fallback
        hash == password
    }
}

fn verify_sha1(password: &str, encoded: &str) -> bool {
    let digest = Sha1::digest(password.as_bytes());
    let expected = STANDARD.encode(digest);
    // encoded is "{SHA}<base64>"
    &encoded[5..] == expected
}

fn verify_apr1(password: &str, hash: &str) -> bool {
    let parts: Vec<&str> = hash.splitn(4, '$').collect();
    // parts[0]="", parts[1]="apr1", parts[2]=salt, parts[3]=expected_hash
    if parts.len() != 4 || parts[1] != "apr1" {
        return false;
    }
    apr1_hash(password, parts[2]) == parts[3]
}

fn apr1_hash(password: &str, salt: &str) -> String {
    use md5::{Md5, Digest as _};
    let pw = password.as_bytes();
    let salt_b = salt.as_bytes();
    let magic = b"$apr1$";

    // Digest B: password + salt + password
    let mut ctx_b = Md5::new();
    ctx_b.update(pw);
    ctx_b.update(salt_b);
    ctx_b.update(pw);
    let digest_b = ctx_b.finalize();

    // Digest A
    let mut ctx_a = Md5::new();
    ctx_a.update(pw);
    ctx_a.update(magic);
    ctx_a.update(salt_b);
    // Add bytes from B based on pw length
    let mut plen = pw.len();
    while plen > 0 {
        let n = plen.min(16);
        ctx_a.update(&digest_b[..n]);
        plen = plen.saturating_sub(16);
    }
    // Bit-length mixing
    let mut plen = pw.len();
    while plen > 0 {
        if plen & 1 != 0 {
            ctx_a.update(b"\x00");
        } else {
            ctx_a.update(&pw[..1]);
        }
        plen >>= 1;
    }
    let mut digest_a = ctx_a.finalize();

    // 1000 iterations
    for i in 0..1000u32 {
        let mut ctx = Md5::new();
        if i & 1 != 0 {
            ctx.update(pw);
        } else {
            ctx.update(digest_a.as_slice());
        }
        if i % 3 != 0 {
            ctx.update(salt_b);
        }
        if i % 7 != 0 {
            ctx.update(pw);
        }
        if i & 1 != 0 {
            ctx.update(digest_a.as_slice());
        } else {
            ctx.update(pw);
        }
        digest_a = ctx.finalize();
    }
    apr1_to64(&digest_a)
}

fn apr1_to64(digest: &[u8]) -> String {
    const CHARS: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let to64 = |v: u32, n: usize| -> String {
        let mut r = String::new();
        let mut v = v;
        for _ in 0..n {
            r.push(CHARS[(v & 0x3f) as usize] as char);
            v >>= 6;
        }
        r
    };
    let mut r = String::new();
    r.push_str(&to64(((digest[0] as u32) << 16) | ((digest[6] as u32) << 8) | digest[12] as u32, 4));
    r.push_str(&to64(((digest[1] as u32) << 16) | ((digest[7] as u32) << 8) | digest[13] as u32, 4));
    r.push_str(&to64(((digest[2] as u32) << 16) | ((digest[8] as u32) << 8) | digest[14] as u32, 4));
    r.push_str(&to64(((digest[3] as u32) << 16) | ((digest[9] as u32) << 8) | digest[15] as u32, 4));
    r.push_str(&to64(((digest[4] as u32) << 16) | ((digest[10] as u32) << 8) | digest[5] as u32, 4));
    r.push_str(&to64(digest[11] as u32, 2));
    r
}

pub fn parse_htpasswd_file(path: &str) -> Result<Vec<(String, String)>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read htpasswd file: {}", e))?;
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(colon_pos) = line.find(':') {
            let username = line[..colon_pos].to_string();
            let hash = line[colon_pos + 1..].to_string();
            entries.push((username, hash));
        }
    }
    Ok(entries)
}

pub fn check_credentials(username: &str, password: &str, entries: &[(String, String)]) -> bool {
    for (user, hash) in entries {
        if user == username {
            return verify_htpasswd(password, hash);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_verify_bcrypt() {
        let hash = bcrypt::hash("testpass", 4).unwrap();
        assert!(verify_htpasswd("testpass", &hash));
        assert!(!verify_htpasswd("wrongpass", &hash));
    }

    #[test]
    fn test_verify_sha1() {
        // {SHA}5en6G6MezRroT3XKqkdPOmY/BfQ= is SHA1 of "secret"
        assert!(verify_htpasswd("secret", "{SHA}5en6G6MezRroT3XKqkdPOmY/BfQ="));
        assert!(!verify_htpasswd("wrong", "{SHA}5en6G6MezRroT3XKqkdPOmY/BfQ="));
    }

    #[test]
    fn test_parse_htpasswd_file() {
        let tmp = TempDir::new().unwrap();
        let hash = bcrypt::hash("mypass", 4).unwrap();
        let content = format!("alice:{}\nbob:ignored\n", hash);
        let path = tmp.path().join(".htpasswd");
        fs::write(&path, &content).unwrap();

        let entries = parse_htpasswd_file(path.to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "alice");
        assert!(verify_htpasswd("mypass", &entries[0].1));
    }

    #[test]
    fn test_check_credentials_ok() {
        let hash = bcrypt::hash("pass123", 4).unwrap();
        let entries = vec![("alice".to_string(), hash)];
        assert!(check_credentials("alice", "pass123", &entries));
    }

    #[test]
    fn test_check_credentials_wrong_user() {
        let hash = bcrypt::hash("pass123", 4).unwrap();
        let entries = vec![("alice".to_string(), hash)];
        assert!(!check_credentials("bob", "pass123", &entries));
    }

    #[test]
    fn test_check_credentials_wrong_pass() {
        let hash = bcrypt::hash("pass123", 4).unwrap();
        let entries = vec![("alice".to_string(), hash)];
        assert!(!check_credentials("alice", "wrongpass", &entries));
    }
}
