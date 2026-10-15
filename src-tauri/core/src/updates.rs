//! Verify update packages against the release key embedded in the application.
use crate::error::{Result, SpiceError};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use minisign_verify::{PublicKey, Signature};
use std::{fs::File, io::Read, path::Path};

const PUBLIC_KEY: &str = include_str!("../../updater.pub");

pub fn verify_artifact(path: &Path, signature: &str) -> Result<()> {
    verify_with_key(path, signature, PUBLIC_KEY)
}

fn verify_with_key(path: &Path, signature: &str, public_key: &str) -> Result<()> {
    let invalid = || {
        SpiceError::User(
            "The update signature could not be verified. Download a fresh update and try again."
                .into(),
        )
    };
    if signature.len() > 4096 {
        return Err(invalid());
    }
    let key = STANDARD.decode(public_key.trim()).map_err(|_| invalid())?;
    let key = PublicKey::decode(std::str::from_utf8(&key).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let signature = STANDARD.decode(signature.trim()).map_err(|_| invalid())?;
    let signature = Signature::decode(std::str::from_utf8(&signature).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let mut verifier = key.verify_stream(&signature).map_err(|_| invalid())?;
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > 512 * 1024 * 1024 {
        return Err("The update package has an unexpected size or type.".into());
    }
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        verifier.update(&buffer[..read]);
    }
    verifier.finalize().map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_signature_is_rejected_before_reading_an_arbitrary_path() {
        assert!(verify_artifact(Path::new("missing.exe"), "not a signature")
            .unwrap_err()
            .to_string()
            .contains("signature"));
    }

    #[test]
    fn release_public_key_is_valid() {
        let key = STANDARD.decode(PUBLIC_KEY.trim()).unwrap();
        PublicKey::decode(std::str::from_utf8(&key).unwrap()).unwrap();
    }

    #[test]
    fn accepts_signed_package_and_rejects_changed_or_truncated_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("update.exe");
        let bytes = include_bytes!("../tests/fixtures/update-signature.txt");
        let signature = include_str!("../tests/fixtures/update-signature.txt.sig");
        std::fs::write(&path, bytes).unwrap();
        verify_artifact(&path, signature).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(verify_artifact(&path, signature).is_err());
        let mut changed = bytes.to_vec();
        changed[0] ^= 1;
        std::fs::write(&path, changed).unwrap();
        assert!(verify_artifact(&path, signature).is_err());
    }
}
