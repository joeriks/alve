use std::fs;
use std::io::Write;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use scrypt::{scrypt, Params};
use tempfile::NamedTempFile;

use crate::{Error, Result};

pub const MAX_ENVELOPE: usize = 16 * 1024 * 1024;
pub const SNAPSHOT: &[u8; 8] = b"ALVEPOC1";
pub const BUNDLE: &[u8; 8] = b"ALVEBND1";

pub fn derive(password: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
    if password.trim().is_empty() || password.len() > 1024 {
        return Err(Error::new(400, "Invalid passphrase."));
    }
    let params =
        Params::new(15, 8, 1, 32).map_err(|_| Error::new(400, "Invalid scrypt parameters."))?;
    let mut key = [0u8; 32];
    scrypt(password.as_bytes(), salt, &params, &mut key)
        .map_err(|_| Error::new(400, "Could not derive vault key."))?;
    Ok(key)
}

pub fn envelope(
    magic: &[u8; 8],
    vault_id: &str,
    salt: &[u8; 16],
    key: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>> {
    if data.len() > MAX_ENVELOPE {
        return Err(Error::new(413, "POC vault limit is 16 MiB."));
    }
    let id = hex_id(vault_id)?;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut header = Vec::with_capacity(52);
    header.extend_from_slice(magic);
    header.extend_from_slice(&id);
    header.extend_from_slice(salt);
    header.extend_from_slice(&nonce);
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| Error::new(400, "Invalid vault key."))?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: data,
                aad: &header,
            },
        )
        .map_err(|_| Error::new(400, "Could not encrypt vault."))?;
    header.extend_from_slice(&ciphertext);
    if header.len() > MAX_ENVELOPE {
        return Err(Error::new(413, "POC vault limit is 16 MiB."));
    }
    Ok(header)
}

pub fn decrypt(
    raw: &[u8],
    magic: &[u8; 8],
    password: &str,
) -> Result<(String, [u8; 16], [u8; 32], Vec<u8>)> {
    if raw.len() < 68 || raw.len() > MAX_ENVELOPE || raw.get(..8) != Some(magic.as_slice()) {
        return Err(Error::new(400, "Unsupported or oversized encrypted file."));
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&raw[8..24]);
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&raw[24..40]);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&raw[40..52]);
    let key = derive(password, &salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| Error::new(400, "Invalid vault key."))?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &raw[52..],
                aad: &raw[..52],
            },
        )
        .map_err(|_| Error::new(401, "Incorrect passphrase or damaged encrypted file."))?;
    Ok((encode_hex(&id), salt, key, plain))
}

pub fn atomic_write(path: &Path, raw: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new(400, "Vault path has no parent."))?;
    fs::create_dir_all(parent).map_err(|_| Error::new(500, "Could not create vault directory."))?;
    let mut temp = NamedTempFile::new_in(parent)
        .map_err(|_| Error::new(500, "Could not create vault snapshot."))?;
    temp.write_all(raw)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| Error::new(500, "Could not write vault snapshot."))?;
    temp.persist(path)
        .map_err(|_| Error::new(500, "Could not replace vault snapshot."))?;
    Ok(())
}

fn hex_id(value: &str) -> Result<[u8; 16]> {
    if value.len() != 32 {
        return Err(Error::new(400, "Invalid vault ID."));
    }
    let mut out = [0u8; 16];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        };
        out[index] = (digit(chunk[0]).ok_or_else(|| Error::new(400, "Invalid vault ID."))? << 4)
            | digit(chunk[1]).ok_or_else(|| Error::new(400, "Invalid vault ID."))?;
    }
    Ok(out)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}
