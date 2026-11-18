//! This install's Ed25519 key for relays: made on the first use and kept in a file of its own
//! (PKCS#8 PEM, written whole to a `.part` file then renamed). The id a relay knows the player by is
//! `mvs_protocol::relay_wire`'s: `r` + the first 32 hex digits of the SHA-256 of the raw public key. On the wire a key,
//! a nonce, a signature and a token are base64 of their raw bytes. This key signs the start only: what goes after it
//! is sealed with the keys of a fresh X25519 exchange of that connection (`mvs_protocol::relay_seal`), never with this
//! one.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey, KeypairBytes};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use mvs_protocol::relay_wire::bad_public_key;
use sha2::{Digest, Sha256};

/// A key file that could not be read or written (the file stays as it was: the sign-in fails and is tried again).
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("the relay key {0} could not be read: {1}")]
    Read(PathBuf, std::io::Error),
    #[error("the relay key {0} could not be written: {1}")]
    Write(PathBuf, String),
}

/// One player's key and the id and public key a relay knows it by (`RelayKey`).
#[derive(Clone)]
pub struct RelayKey {
    signing: SigningKey,
    raw: [u8; 32],
    id: String,
}

impl std::fmt::Debug for RelayKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayKey").field("id", &self.id).finish_non_exhaustive()
    }
}

/// `keyId`: `r` + the first 32 hex digits of the SHA-256 of the raw public key (the wire's).
pub use mvs_protocol::relay_wire::key_id;

/// `keyFingerprint`: what a direct host's code names of its key: the first 8 bytes of the SHA-256 of the raw public
/// key, hex.
pub fn key_fingerprint(raw: &[u8]) -> String {
    Sha256::digest(raw)[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `verifySig`: whether `sig` (base64) is the key's (raw) signature of `bytes`; false for a key that is no usable one
/// (`bad_public_key`) or a signature that does not decode.
pub fn verify_sig(raw: &[u8; 32], bytes: &[u8], sig: &str) -> bool {
    if bad_public_key(raw) {
        return false;
    }
    let Ok(key) = VerifyingKey::from_bytes(raw) else { return false };
    let Ok(sig) = B64.decode(sig) else { return false };
    let Ok(sig) = Signature::from_slice(&sig) else { return false };
    key.verify(bytes, &sig).is_ok()
}

impl RelayKey {
    fn of(signing: SigningKey) -> RelayKey {
        let raw = signing.verifying_key().to_bytes();
        RelayKey { id: key_id(&raw), raw, signing }
    }

    /// `freshKey`: a key for one connection only.
    pub fn fresh() -> RelayKey {
        let mut secret = [0u8; 32];
        rand::fill(&mut secret);
        RelayKey::of(SigningKey::from_bytes(&secret))
    }

    /// `loadKey`: the key in `file`, made there when there is none. A file that does not parse as an Ed25519 key is set
    /// aside (`.bad`, else the first free `.bad-<n>`) and a new key made. A file that cannot be read throws, and stays.
    pub fn load(file: &Path) -> Result<RelayKey, KeyError> {
        // (bytes that are not UTF-8 parse as no key: set aside as a bad one, as a utf8 read then parse does)
        let text = match std::fs::read(file) {
            Ok(b) => Some(String::from_utf8(b).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(KeyError::Read(file.to_owned(), e)),
        };
        if let Some(text) = text {
            if let Ok(k) = SigningKey::from_pkcs8_pem(&text) {
                return Ok(RelayKey::of(k));
            }
            std::fs::rename(file, bad_name(file)).map_err(|e| KeyError::Write(file.to_owned(), e.to_string()))?;
        }
        let key = RelayKey::fresh();
        // (PKCS#8 v1, the private key alone, as Node's export writes it)
        let doc = KeypairBytes { secret_key: key.signing.to_bytes(), public_key: None };
        let pem = doc
            .to_pkcs8_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
            .map_err(|e| KeyError::Write(file.to_owned(), e.to_string()))?;
        let write = || -> std::io::Result<()> {
            if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            let part = with_suffix(file, ".part");
            std::fs::write(&part, pem.as_bytes())?;
            std::fs::rename(&part, file)
        };
        write().map_err(|e| KeyError::Write(file.to_owned(), e.to_string()))?;
        Ok(key)
    }

    /// `r` + 32 hex digits.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The raw public key.
    pub fn raw(&self) -> &[u8; 32] {
        &self.raw
    }

    /// The raw public key, base64.
    pub fn public_key(&self) -> String {
        B64.encode(self.raw)
    }

    /// The Ed25519 signature of `bytes`, base64.
    pub fn sign(&self, bytes: &[u8]) -> String {
        B64.encode(self.signing.sign(bytes).to_bytes())
    }
}

fn with_suffix(file: &Path, suffix: &str) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Where a key file that holds no key is set aside: `.bad`, else the first free `.bad-<n>` (none is overwritten).
fn bad_name(file: &Path) -> PathBuf {
    let first = with_suffix(file, ".bad");
    if !first.exists() {
        return first;
    }
    (2..).map(|n| with_suffix(file, &format!(".bad-{n}"))).find(|p| !p.exists()).expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_made_kept_and_set_aside_when_bad() {
        let dir = std::env::temp_dir().join(format!("mvs-net-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("relay.key");
        let a = RelayKey::load(&file).expect("made");
        let b = RelayKey::load(&file).expect("read again");
        assert_eq!(a.id(), b.id());
        assert!(a.id().starts_with('r') && a.id().len() == 33);
        let sig = a.sign(b"transcript");
        assert!(verify_sig(a.raw(), b"transcript", &sig));
        assert!(!verify_sig(a.raw(), b"another", &sig));
        std::fs::write(&file, "not a key").expect("written");
        let c = RelayKey::load(&file).expect("made anew");
        assert_ne!(c.id(), a.id());
        assert!(with_suffix(&file, ".bad").exists());
        std::fs::write(&file, [0xffu8, 0xfe, 0x00, 0x80]).expect("written");
        let d = RelayKey::load(&file).expect("made anew from bytes that are not text");
        assert_ne!(d.id(), c.id());
        assert!(with_suffix(&file, ".bad-2").exists());
        std::fs::remove_dir_all(&dir).expect("removed");
    }
}
