//! argon2id password hashes (D-50, BR-AUTH-01). PHC strings in `users.password_hash`; never a plain password.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use std::sync::LazyLock;

/// Minimum length of a new password (OP-AUTH-03 WeakPassword). 07 names the error but no length; 10 is the demo
/// default.
pub const MIN_LEN: usize = 10;

/// Hashes with a fresh random salt.
pub fn hash(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::fill(&mut salt);
    hash_with_salt(password, &salt)
}

/// Hashes with the given salt bytes. The seed tool passes salts from its seeded generator, so the same seed gives
/// the same database (T-03-02).
pub fn hash_with_salt(password: &str, salt: &[u8; 16]) -> String {
    let salt = SaltString::encode_b64(salt).expect("16 bytes is a valid salt");
    Argon2::default().hash_password(password.as_bytes(), &salt).expect("argon2id hash").to_string()
}

/// True when `password` matches the PHC string. A malformed hash never matches.
pub fn verify(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// A hash no password matches in practice. Sign-in verifies against it for an unknown username, so the answer
/// takes the same time whether the user exists or not (BR-AUTH-01: no hint which part failed).
pub static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| hash_with_salt("no-such-user-placeholder", &[7u8; 16]));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2id_round_trip() {
        let h = hash("demo-demo-demo");
        assert!(h.starts_with("$argon2id$"), "{h}");
        assert!(verify("demo-demo-demo", &h));
        assert!(!verify("demo-demo-dem0", &h));
        assert!(!verify("x", "not a hash"));
        assert_ne!(hash("same"), hash("same"), "fresh salt each time");
        assert_eq!(hash_with_salt("same", &[1; 16]), hash_with_salt("same", &[1; 16]));
    }
}
