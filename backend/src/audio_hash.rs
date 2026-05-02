use sha2::{Digest, Sha256};
use thiserror::Error;

pub const AUDIO_CONTENT_HASH_ALGORITHM_ID: &str = "sha256:chunk-sha256-raw-concat:v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AudioContentHashError {
    #[error("chunk digest at index {index} is not lowercase SHA-256 hex")]
    InvalidChunkDigest { index: usize },
}

pub fn compose_audio_content_hash_hex<I, S>(
    chunk_sha256_hexes: I,
) -> Result<String, AudioContentHashError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut hasher = Sha256::new();
    for (index, chunk_sha256_hex) in chunk_sha256_hexes.into_iter().enumerate() {
        let digest = decode_lowercase_sha256_hex(chunk_sha256_hex.as_ref(), index)?;
        hasher.update(digest);
    }

    Ok(hex_lower(&hasher.finalize()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

pub fn validate_lowercase_sha256_hex(value: &str) -> Result<(), AudioContentHashError> {
    decode_lowercase_sha256_hex(value, 0).map(|_| ())
}

fn decode_lowercase_sha256_hex(
    value: &str,
    index: usize,
) -> Result<[u8; 32], AudioContentHashError> {
    let bytes = value.as_bytes();
    if bytes.len() != 64 {
        return Err(AudioContentHashError::InvalidChunkDigest { index });
    }

    let mut decoded = [0_u8; 32];
    for (target, pair) in decoded.iter_mut().zip(bytes.chunks_exact(2)) {
        let high = decode_hex_nibble(pair[0], index)?;
        let low = decode_hex_nibble(pair[1], index)?;
        *target = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_hex_nibble(value: u8, index: usize) -> Result<u8, AudioContentHashError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(AudioContentHashError::InvalidChunkDigest { index }),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
