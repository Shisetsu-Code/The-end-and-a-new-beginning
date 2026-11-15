//! Native protocol reconstructed from standalone info/belatra/codigo; no old runtime dependency.
use aes::{
    Aes128,
    cipher::{Block, BlockEncrypt, KeyInit},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

fn transform(secret: &str, prefix: &[u8; 8], bytes: &mut [u8]) {
    let mut material = Block::<Aes128>::default();
    let key = secret.as_bytes();
    let len = key.len().min(material.len());
    material[..len].copy_from_slice(&key[..len]);
    Aes128::new(&material).encrypt_block(&mut material);
    let aes = Aes128::new(&material);
    for (index, chunk) in bytes.chunks_mut(16).enumerate() {
        let mut stream = Block::<Aes128>::default();
        stream[..8].copy_from_slice(prefix);
        stream[8..].copy_from_slice(&(index as u64).to_be_bytes());
        aes.encrypt_block(&mut stream);
        for (byte, mask) in chunk.iter_mut().zip(stream.iter()) {
            *byte ^= mask;
        }
    }
}

pub(crate) fn seal(payload: &[u8], secret: &str, prefix: [u8; 8]) -> String {
    let mut result = prefix.to_vec();
    result.extend_from_slice(payload);
    transform(secret, &prefix, &mut result[8..]);
    STANDARD.encode(result)
}

pub(crate) fn open(envelope: Value, secret: &str) -> Result<Value> {
    let Some(cipher) = envelope.get("d") else {
        ensure!(envelope.is_object(), "invalid Belatra response envelope");
        return Ok(envelope);
    };
    let mut bytes = STANDARD.decode(cipher.as_str().context("invalid encrypted response")?)?;
    let prefix: [u8; 8] = bytes
        .get(..8)
        .context("short encrypted response")?
        .try_into()?;
    transform(secret, &prefix, &mut bytes[8..]);
    serde_json::from_slice(&bytes[8..]).context("invalid native Belatra response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_official_client_envelope() {
        let prefix = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            seal(
                "demo fixture unicode ñ".as_bytes(),
                "sample-test-key",
                prefix
            ),
            "AQIDBAUGBwhoMOT7q7tDGD2QFcODCjV0jTmZ4aQbQQ=="
        );
        let payload = json!({"q":"enter","text":"á"});
        let cipher = seal(
            &serde_json::to_vec(&payload).unwrap(),
            "sample-test-key",
            prefix,
        );
        assert_eq!(
            open(json!({"d":cipher}), "sample-test-key").unwrap(),
            payload
        );
        assert!(open(json!({"d":"AQID"}), "sample-test-key").is_err());
    }
}
