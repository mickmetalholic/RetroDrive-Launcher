#[path = "../update_common.rs"]
mod update_common;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use std::{env, fs, path::PathBuf};
use update_common::{sha256_file, UpdateManifest, UpdatePackage, UPDATE_SCHEMA_VERSION};

fn fail(message: impl AsRef<str>) -> ! {
    eprintln!("{}", message.as_ref());
    std::process::exit(1);
}

fn signing_key() -> Result<SigningKey, String> {
    let encoded = env::var("RETRODRIVE_SIGNING_PRIVATE_KEY")
        .map_err(|_| "RETRODRIVE_SIGNING_PRIVATE_KEY is not set".to_string())?;
    let bytes: [u8; 32] = STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("Signing key is not valid Base64: {error}"))?
        .try_into()
        .map_err(|_| "Signing key must contain exactly 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn value(args: &[String], name: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("Missing required argument {name}"))?;
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("Missing value for {name}"))
}

fn create_manifest(args: &[String]) -> Result<(), String> {
    let version = value(args, "--version")?;
    let tag = value(args, "--tag")?;
    let repository = value(args, "--repository")?;
    let archive = PathBuf::from(value(args, "--archive")?);
    let output = PathBuf::from(value(args, "--output")?);
    let notes = value(args, "--notes")?;
    let published_at = value(args, "--published-at")?;
    let archive_name = archive
        .file_name()
        .ok_or_else(|| "Archive path has no file name".to_string())?
        .to_string_lossy();
    let size = archive
        .metadata()
        .map_err(|error| format!("Unable to inspect {}: {error}", archive.display()))?
        .len();
    let mut manifest = UpdateManifest {
        schema_version: UPDATE_SCHEMA_VERSION,
        version,
        published_at,
        notes,
        package: UpdatePackage {
            url: format!("https://github.com/{repository}/releases/download/{tag}/{archive_name}"),
            sha256: sha256_file(&archive)?,
            size,
        },
        signature: String::new(),
    };
    manifest.validate()?;
    let signature = signing_key()?.sign(&manifest.unsigned_bytes()?);
    manifest.signature = STANDARD.encode(signature.to_bytes());
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Unable to serialize signed manifest: {error}"))?;
    fs::write(&output, format!("{json}\n"))
        .map_err(|error| format!("Unable to write {}: {error}", output.display()))?;
    manifest.verify()?;
    println!("Created signed manifest at {}", output.display());
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("public-key") => {
            println!(
                "{}",
                STANDARD.encode(signing_key()?.verifying_key().to_bytes())
            );
            Ok(())
        }
        Some("create") => create_manifest(&args[1..]),
        _ => Err(
            "Usage: release-manifest public-key | create --version ... --tag ... --repository ... --archive ... --output ... --notes ... --published-at ..."
                .to_string(),
        ),
    }
}

fn main() {
    if let Err(error) = run() {
        fail(error);
    }
}
