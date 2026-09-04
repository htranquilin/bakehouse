//! SA passwords live in the macOS Keychain, one entry per instance.
//! Note: the password is also visible in `container inspect` env output while
//! a container runs — acceptable for a local dev tool, documented in findings.

use crate::error::{AppError, Result};

const SERVICE: &str = "com.bakehouse.sa-password";

/// SQL Server password policy: 8-128 chars, 3 of 4 character classes.
pub fn generate_password() -> String {
    use rand::seq::IndexedRandom;
    let mut rng = rand::rng();
    let upper: Vec<char> = ('A'..='Z').collect();
    let lower: Vec<char> = ('a'..='z').collect();
    let digit: Vec<char> = ('0'..='9').collect();
    let all: Vec<char> = upper.iter().chain(&lower).chain(&digit).copied().collect();
    let mut pw: Vec<char> = vec![
        *upper.choose(&mut rng).unwrap(),
        *lower.choose(&mut rng).unwrap(),
        *digit.choose(&mut rng).unwrap(),
    ];
    for _ in 0..21 {
        pw.push(*all.choose(&mut rng).unwrap());
    }
    // No shuffle needed for strength here; positions are not attacker-visible.
    pw.into_iter().collect()
}

pub fn store(instance_id: &str, password: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, instance_id)
        .and_then(|e| e.set_password(password))
        .map_err(|e| AppError::Internal(format!("keychain write failed: {e}")))
}

pub fn get(instance_id: &str) -> Result<String> {
    keyring::Entry::new(SERVICE, instance_id)
        .and_then(|e| e.get_password())
        .map_err(|e| AppError::Internal(format!("keychain read failed: {e}")))
}

pub fn delete(instance_id: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, instance_id) {
        let _ = entry.delete_credential();
    }
}
