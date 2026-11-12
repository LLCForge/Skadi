//! Ed25519 imzaları: başkasından gelen profilleri, tweak paketlerini, eklentileri ve kurum
//! politikalarını doğrulamak için. Anahtarlar ve imzalar onaltılık (hex) metin olarak tutulur:
//! gizli anahtar 32, açık anahtar 32, imza 64 bayt.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::error::{Error, Result};

fn bad(what: &str) -> Error {
    Error::Signature(what.to_owned())
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Yeni anahtar çifti: (gizli, açık) hex metinleri. Rastgelelik işletim sisteminden gelir.
pub fn generate() -> Result<(String, String)> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| Error::State(format!("random: {e}")))?;
    let key = SigningKey::from_bytes(&secret);
    Ok((to_hex(&key.to_bytes()), to_hex(&key.verifying_key().to_bytes())))
}

pub fn sign(secret_hex: &str, data: &[u8]) -> Result<String> {
    let secret = from_hex::<32>(secret_hex).ok_or_else(|| bad("invalid secret key"))?;
    Ok(to_hex(&SigningKey::from_bytes(&secret).sign(data).to_bytes()))
}

/// İmzayı doğrular (katı doğrulama: zayıf açık anahtarları ve değiştirilebilir imzaları reddeder).
pub fn verify(public_hex: &str, data: &[u8], signature_hex: &str) -> Result<()> {
    let public = from_hex::<32>(public_hex).ok_or_else(|| bad("invalid public key"))?;
    let key = VerifyingKey::from_bytes(&public).map_err(|_| bad("invalid public key"))?;
    let signature = from_hex::<64>(signature_hex).ok_or_else(|| bad("invalid signature"))?;
    key.verify_strict(data, &Signature::from_bytes(&signature))
        .map_err(|_| bad("signature does not match"))
}

/// Bir dosyanın imza dosyası (`dosya.sig`).
pub fn signature_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".sig");
    PathBuf::from(name)
}

/// Dosyayı ve yanındaki `.sig` imzasını verilen açık anahtar dosyasıyla doğrular; içeriği döndürür.
/// İçerik bir kez okunur, böylece doğrulama ile kullanım arasında dosya değiştirilemez.
pub fn read_verified(file: &Path, public_key: &Path) -> Result<Vec<u8>> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |e: std::io::Error| bad(&format!("{}: {e}", path.display()))
    };
    let data = std::fs::read(file).map_err(io(file))?;
    let sig_path = signature_path(file);
    let signature = std::fs::read_to_string(&sig_path).map_err(io(&sig_path))?;
    let public = std::fs::read_to_string(public_key).map_err(io(public_key))?;
    verify(&public, &data, &signature)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_rejects_changes() {
        let (secret, public) = generate().unwrap();
        assert_eq!(secret.len(), 64);
        assert_eq!(public.len(), 64);
        let signature = sign(&secret, b"apps = [\"git\"]").unwrap();
        assert!(verify(&public, b"apps = [\"git\"]", &signature).is_ok());
        assert!(verify(&public, b"apps = [\"evil\"]", &signature).is_err());
        let (_, other) = generate().unwrap();
        assert!(verify(&other, b"apps = [\"git\"]", &signature).is_err());
        assert!(verify(&public, b"x", "zz").is_err());
        assert_eq!(from_hex::<2>("0aff"), Some([0x0a, 0xff]));
        assert_eq!(from_hex::<2>("0afg"), None);
    }
}
