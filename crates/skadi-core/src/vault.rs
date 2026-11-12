//! Durum dosyasının isteğe bağlı şifrelenmesi (`settings.toml`: `[state] encrypt = true`).
//!
//! Dosya ChaCha20-Poly1305 ile şifrelenir. 32 baytlık anahtar işletim sisteminin kasasında durur:
//! Windows'ta DPAPI ile kullanıcıya bağlı olarak korunan bir dosyada, macOS'ta Anahtar Zinciri'nde,
//! Linux'ta Secret Service'te (`secret-tool`). Anahtar mümkün olan her yerde araca standart girişten
//! verilir, böylece süreç listesinde görünmez.

use std::{
    io::Write,
    process::{Command, Stdio},
};

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};

use crate::{
    error::{Error, Result},
    exec::find_program,
    platform::Os,
    signing::{from_hex, to_hex},
};

const MAGIC: &[u8] = b"SKADIENC1";
const SERVICE: &str = "org.llcforge.skadi.state";

fn fail(detail: impl std::fmt::Display) -> Error {
    Error::State(format!("state encryption: {detail}"))
}

pub fn is_encrypted(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

pub fn encrypt(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(fail)?;
    let cipher = ChaCha20Poly1305::new(key.into());
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: MAGIC })
        .map_err(|_| fail("encryption failed"))?;
    Ok([MAGIC, &nonce, &sealed].concat())
}

pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    let body = data
        .strip_prefix(MAGIC)
        .ok_or_else(|| fail("not an encrypted state file"))?;
    if body.len() < 12 + 16 {
        return Err(fail("file is truncated"));
    }
    let (nonce, sealed) = body.split_at(12);
    ChaCha20Poly1305::new(key.into())
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: MAGIC,
            },
        )
        .map_err(|_| fail("wrong key or the file was changed"))
}

/// Programı çalıştırır, `input` standart girişe yazılır; başarılıysa çıktı döner.
fn run(program: &str, args: &[&str], input: Option<&[u8]>) -> Result<String> {
    let path = find_program(program).ok_or(Error::ProgramNotFound("key store tool"))?;
    let mut child = Command::new(path)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(fail)?;
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(data).map_err(fail)?;
    }
    let output = child.wait_with_output().map_err(fail)?;
    if !output.status.success() {
        return Err(fail(format!("{program} failed")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

const DPAPI: &str = "Add-Type -AssemblyName System.Security; $in = [Console]::In.ReadToEnd().Trim(); \
     [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::{op}([Convert]::FromBase64String($in), $null, 'CurrentUser'))";

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let clean: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        let mut count: u32 = 0;
        for &c in chunk {
            if c == b'=' {
                break;
            }
            n = (n << 6) | u32::from(value(c)?);
            count += 1;
        }
        n <<= 6 * (4 - count);
        for i in 0..count.saturating_sub(1) {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// Kasadaki anahtar; `create` ise yoksa yenisini üretip kasaya koyar.
pub fn state_key(os: Os, create: bool) -> Result<[u8; 32]> {
    let parse = |hex: &str| from_hex::<32>(hex).ok_or_else(|| fail("stored key is invalid"));
    match os {
        Os::Windows => {
            let file = crate::paths::data_dir(os)
                .ok_or_else(|| fail("no data directory"))?
                .join("state.key.dpapi");
            if let Ok(protected) = std::fs::read_to_string(&file) {
                let plain = run(
                    "powershell",
                    &[
                        "-NoProfile",
                        "-NonInteractive",
                        "-Command",
                        &DPAPI.replace("{op}", "Unprotect"),
                    ],
                    Some(protected.as_bytes()),
                )?;
                let bytes = unbase64(&plain).ok_or_else(|| fail("stored key is invalid"))?;
                return bytes.try_into().map_err(|_| fail("stored key is invalid"));
            }
            if !create {
                return Err(fail("the key is missing"));
            }
            let key = new_key()?;
            let protected = run(
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    &DPAPI.replace("{op}", "Protect"),
                ],
                Some(base64(&key).as_bytes()),
            )?;
            std::fs::write(&file, protected).map_err(fail)?;
            Ok(key)
        }
        Os::Macos => {
            if let Ok(hex) = run(
                "security",
                &["find-generic-password", "-a", "skadi", "-s", SERVICE, "-w"],
                None,
            ) {
                return parse(&hex);
            }
            if !create {
                return Err(fail("the key is missing from the Keychain"));
            }
            let key = new_key()?;
            // `security` parolayı yalnızca bağımsız değişken olarak alır; süre milisaniyelerle sınırlıdır.
            run(
                "security",
                &[
                    "add-generic-password",
                    "-a",
                    "skadi",
                    "-s",
                    SERVICE,
                    "-U",
                    "-w",
                    &to_hex(&key),
                ],
                None,
            )?;
            Ok(key)
        }
        Os::Linux => {
            if let Ok(hex) = run("secret-tool", &["lookup", "service", SERVICE], None)
                && !hex.is_empty()
            {
                return parse(&hex);
            }
            if !create {
                return Err(fail("the key is missing from the Secret Service"));
            }
            let key = new_key()?;
            run(
                "secret-tool",
                &["store", "--label=Skadi state", "service", SERVICE],
                Some(to_hex(&key).as_bytes()),
            )?;
            Ok(key)
        }
    }
}

fn new_key() -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(fail)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_detects_tampering() {
        let key = [7u8; 32];
        let sealed = encrypt(&key, b"[tweaks]\n").unwrap();
        assert!(is_encrypted(&sealed));
        assert_eq!(decrypt(&key, &sealed).unwrap(), b"[tweaks]\n");
        let mut changed = sealed.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(decrypt(&key, &changed).is_err());
        assert!(decrypt(&[8u8; 32], &sealed).is_err());
        assert!(!is_encrypted(b"[tweaks]"));
    }

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"a", b"ab", b"abc", &[0u8, 255, 16, 32, 64][..]] {
            assert_eq!(unbase64(&base64(data)).unwrap(), data);
        }
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
    }
}
