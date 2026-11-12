//! JPEG ve PNG dosyalarından gizlilik açısından hassas meta verileri (EXIF/GPS, XMP, IPTC,
//! yorumlar, metin parçaları) silme. Görüntü verisine ve renk profiline dokunulmaz.

use crate::error::{Error, Result};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Saldırgan ya da hatalı dosyalara karşı üst sınır.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
}

pub fn detect(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(PNG_SIGNATURE) {
        Some(Format::Png)
    } else {
        None
    }
}

fn malformed(what: &str) -> Error {
    Error::Media(format!("malformed {what} file"))
}

/// Meta verisi silinmiş kopya.
pub fn strip(bytes: &[u8]) -> Result<Vec<u8>> {
    match detect(bytes) {
        Some(Format::Jpeg) => strip_jpeg(bytes),
        Some(Format::Png) => strip_png(bytes),
        None => Err(Error::Media("unsupported file type (only JPEG and PNG)".into())),
    }
}

/// APP0 (JFIF), APP2 (ICC renk profili) ve APP14 (Adobe renk dönüşümü) korunur; diğer APPn
/// bölümleri (EXIF/XMP/IPTC ...) ve yorumlar (COM) silinir. Tarama verisi (SOS) aynen kopyalanır.
fn strip_jpeg(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..2]);
    let mut i = 2;
    loop {
        // Dolgu baytlarını atla.
        while bytes.get(i) == Some(&0xFF) && bytes.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        let (Some(&0xFF), Some(&marker)) = (bytes.get(i), bytes.get(i + 1)) else {
            return Err(malformed("JPEG"));
        };
        // Uzunluğu olmayan işaretler.
        if marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
            if marker == 0xD9 {
                return Ok(out);
            }
            continue;
        }
        let length = bytes
            .get(i + 2..i + 4)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            .filter(|len| *len >= 2)
            .ok_or_else(|| malformed("JPEG"))?;
        let end = i + 2 + length;
        let segment = bytes.get(i..end).ok_or_else(|| malformed("JPEG"))?;
        let keep = match marker {
            0xE0 | 0xE2 | 0xEE => true,
            0xE1 | 0xE3..=0xED | 0xEF | 0xFE => false,
            _ => true,
        };
        if keep {
            out.extend_from_slice(segment);
        }
        i = end;
        if marker == 0xDA {
            // Başlangıç taraması: geri kalan sıkıştırılmış veri ve EOI aynen kopyalanır.
            out.extend_from_slice(&bytes[i..]);
            return Ok(out);
        }
    }
}

/// Metin (tEXt, zTXt, iTXt), EXIF (eXIf) ve zaman (tIME) parçaları silinir.
fn strip_png(bytes: &[u8]) -> Result<Vec<u8>> {
    const DROP: [&[u8; 4]; 5] = [b"tEXt", b"zTXt", b"iTXt", b"eXIf", b"tIME"];
    let mut out = PNG_SIGNATURE.to_vec();
    let mut i = PNG_SIGNATURE.len();
    while i < bytes.len() {
        let header = bytes.get(i..i + 8).ok_or_else(|| malformed("PNG"))?;
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let end = i
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| malformed("PNG"))?;
        let kind = &header[4..8];
        if !DROP.iter().any(|drop| *drop == kind) {
            out.extend_from_slice(&bytes[i..end]);
        }
        i = end;
        if kind == b"IEND" {
            return Ok(out);
        }
    }
    Err(malformed("PNG"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn strips_exif_and_comments_from_jpeg() {
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(0xE0, b"JFIF\0data"));
        jpeg.extend(segment(0xE1, b"Exif\0\0GPS 41.0,29.0"));
        jpeg.extend(segment(0xE2, b"ICC_PROFILE\0x"));
        jpeg.extend(segment(0xFE, b"secret comment"));
        jpeg.extend(segment(0xDB, b"quant"));
        jpeg.extend(segment(0xDA, b"scan"));
        jpeg.extend_from_slice(&[1, 2, 3, 0xFF, 0x00, 4, 0xFF, 0xD9]);

        let clean = strip(&jpeg).unwrap();
        let text = String::from_utf8_lossy(&clean);
        assert!(!text.contains("GPS") && !text.contains("secret"));
        assert!(text.contains("JFIF") && text.contains("ICC_PROFILE") && text.contains("quant"));
        assert!(
            clean.ends_with(&[1, 2, 3, 0xFF, 0x00, 4, 0xFF, 0xD9]),
            "görüntü verisi aynen kalmalı"
        );
        assert_eq!(strip(&clean).unwrap(), clean, "ikinci temizlik bir şey değiştirmez");
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out
    }

    #[test]
    fn strips_text_chunks_from_png() {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"tEXt", b"Author\0Jane"));
        png.extend(chunk(b"eXIf", b"gps"));
        png.extend(chunk(b"IDAT", b"pixels"));
        png.extend(chunk(b"IEND", b""));
        let clean = strip(&png).unwrap();
        let text = String::from_utf8_lossy(&clean);
        assert!(!text.contains("Jane") && !text.contains("gps"));
        assert!(text.contains("IHDR") && text.contains("pixels") && text.contains("IEND"));
    }

    #[test]
    fn rejects_truncated_and_unknown_files_without_panicking() {
        assert!(strip(b"not an image").is_err());
        assert!(strip(&[0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF]).is_err());
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, b'I', b'D', b'A', b'T']);
        assert!(strip(&png).is_err());
    }
}
