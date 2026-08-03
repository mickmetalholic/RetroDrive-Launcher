#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub const UPDATE_MANIFEST_URL: &str =
    "https://github.com/mickmetalholic/RetroDrive-Launcher/releases/latest/download/latest.json";
pub const UPDATE_REPOSITORY: &str = "mickmetalholic/RetroDrive-Launcher";
pub const UPDATE_PUBLIC_KEY_BASE64: &str = "ySu46MFrQT8VnM7S7U7uuh1a6hh8o4J89UvzV3s1fwE=";
pub const UPDATE_SCHEMA_VERSION: u32 = 1;
pub const MAX_UPDATE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePackage {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub version: String,
    pub published_at: String,
    pub notes: String,
    pub package: UpdatePackage,
    pub signature: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsignedManifest<'a> {
    schema_version: u32,
    version: &'a str,
    published_at: &'a str,
    notes: &'a str,
    package: &'a UpdatePackage,
}

impl UpdateManifest {
    pub fn unsigned_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&UnsignedManifest {
            schema_version: self.schema_version,
            version: &self.version,
            published_at: &self.published_at,
            notes: &self.notes,
            package: &self.package,
        })
        .map_err(|error| format!("Unable to serialize update manifest: {error}"))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UPDATE_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported update manifest schema {}",
                self.schema_version
            ));
        }
        Version::parse(&self.version)
            .map_err(|error| format!("Invalid update version: {error}"))?;
        if self.notes.len() > 8_000 {
            return Err("Update notes are unexpectedly large".to_string());
        }
        let expected_prefix = format!("https://github.com/{UPDATE_REPOSITORY}/releases/download/");
        if !self.package.url.starts_with(&expected_prefix) || !self.package.url.ends_with(".zip") {
            return Err("Update package URL is outside the trusted repository".to_string());
        }
        if self.package.size == 0 || self.package.size > MAX_UPDATE_BYTES {
            return Err("Update package size is outside the accepted range".to_string());
        }
        if self.package.sha256.len() != 64
            || !self
                .package
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Update package SHA-256 is invalid".to_string());
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), String> {
        self.validate()?;
        let public_key: [u8; 32] = STANDARD
            .decode(UPDATE_PUBLIC_KEY_BASE64)
            .map_err(|error| format!("Invalid embedded update public key: {error}"))?
            .try_into()
            .map_err(|_| "Embedded update public key must be 32 bytes".to_string())?;
        let signature: [u8; 64] = STANDARD
            .decode(&self.signature)
            .map_err(|error| format!("Invalid update signature encoding: {error}"))?
            .try_into()
            .map_err(|_| "Update signature must be 64 bytes".to_string())?;
        let verifying_key = VerifyingKey::from_bytes(&public_key)
            .map_err(|error| format!("Invalid embedded update public key: {error}"))?;
        verifying_key
            .verify_strict(&self.unsigned_bytes()?, &Signature::from_bytes(&signature))
            .map_err(|_| "Update manifest signature verification failed".to_string())
    }

    pub fn is_newer_than(&self, current: &str) -> Result<bool, String> {
        let available = Version::parse(&self.version)
            .map_err(|error| format!("Invalid update version: {error}"))?;
        let installed = Version::parse(current)
            .map_err(|error| format!("Invalid installed version: {error}"))?;
        Ok(available > installed)
    }
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("Unable to open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Unable to read {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn verify_package(path: &Path, manifest: &UpdateManifest) -> Result<(), String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("Unable to inspect {}: {error}", path.display()))?;
    if metadata.len() != manifest.package.size {
        return Err(format!(
            "Update package size mismatch: expected {}, received {}",
            manifest.package.size,
            metadata.len()
        ));
    }
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(&manifest.package.sha256) {
        return Err("Update package SHA-256 verification failed".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_manifest() -> UpdateManifest {
        UpdateManifest {
            schema_version: 1,
            version: "1.2.3".to_string(),
            published_at: "2026-08-03T00:00:00Z".to_string(),
            notes: "Example".to_string(),
            package: UpdatePackage {
                url: "https://github.com/mickmetalholic/RetroDrive-Launcher/releases/download/v1.2.3/RetroDrive-Launcher-v1.2.3.zip".to_string(),
                sha256: "a".repeat(64),
                size: 123,
            },
            signature: STANDARD.encode([0_u8; 64]),
        }
    }

    #[test]
    fn accepts_only_newer_semver() {
        let manifest = example_manifest();
        assert!(manifest.is_newer_than("1.2.2").unwrap());
        assert!(!manifest.is_newer_than("1.2.3").unwrap());
        assert!(!manifest.is_newer_than("2.0.0").unwrap());
    }

    #[test]
    fn rejects_packages_outside_the_release_repository() {
        let mut manifest = example_manifest();
        manifest.package.url = "https://example.com/update.zip".to_string();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn unsigned_payload_does_not_include_the_signature() {
        let manifest = example_manifest();
        let payload = String::from_utf8(manifest.unsigned_bytes().unwrap()).unwrap();
        assert!(!payload.contains("signature"));
        assert!(payload.contains("1.2.3"));
    }
}
