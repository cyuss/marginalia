//! Content identity.
//!
//! WHY checksums are a domain type rather than a `String`: every device write
//! is verified by comparing one of these afterwards, and the immutability of a
//! user's original PDF is asserted by comparing one before and after. Making
//! the comparison a method on a type keeps that check from being written as a
//! sloppy string compare somewhere.

use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Checksum(String);

impl Checksum {
    /// Compute a SHA-256 over in-memory bytes.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(format!("{:x}", hasher.finalize()))
    }

    /// Adopt an already-computed digest (e.g. streamed from a large file).
    pub fn parse(s: impl Into<String>) -> Result<Self, CoreError> {
        let s = s.into();
        let valid = s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit());
        if !valid {
            return Err(CoreError::InvalidChecksum(s));
        }
        Ok(Self(s.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Verify that content matches this checksum.
    ///
    /// Returns a structured error rather than a bool so the caller cannot
    /// accidentally ignore the result the way `if !matches {}` invites.
    pub fn verify(&self, actual: &Checksum) -> Result<(), CoreError> {
        if self == actual {
            Ok(())
        } else {
            Err(CoreError::ChecksumMismatch {
                expected: self.0.clone(),
                actual: actual.0.clone(),
            })
        }
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // SHA-256 of the empty input.
        assert_eq!(
            Checksum::of_bytes(b"").as_str(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn identical_content_matches() {
        let a = Checksum::of_bytes(b"%PDF-1.7 hello");
        let b = Checksum::of_bytes(b"%PDF-1.7 hello");
        assert!(a.verify(&b).is_ok());
    }

    #[test]
    fn a_single_flipped_byte_is_caught() {
        let a = Checksum::of_bytes(b"%PDF-1.7 hello");
        let b = Checksum::of_bytes(b"%PDF-1.7 hellO");
        assert!(a.verify(&b).is_err());
    }

    #[test]
    fn rejects_malformed_digests() {
        assert!(Checksum::parse("not-a-checksum").is_err());
        assert!(Checksum::parse("abc").is_err());
        assert!(Checksum::parse("g".repeat(64)).is_err());
        assert!(Checksum::parse("a".repeat(64)).is_ok());
    }
}
