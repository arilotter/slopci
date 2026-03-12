use anyhow::Result;
use hkdf::Hkdf;
use sha2::Sha256;
use std::io::Read;

/// Derive a deterministic age identity (private key) for a specific repo+action combo.
///
/// Uses HKDF to derive 32 bytes of keying material from the master key,
/// then encodes as a bech32 age secret key string.
pub fn derive_identity(
    master_key: &[u8],
    owner: &str,
    repo: &str,
    action: &str,
) -> Result<age::x25519::Identity> {
    let info = format!("nixci-v1/{owner}/{repo}/{action}");

    let hkdf = Hkdf::<Sha256>::new(None, master_key);
    let mut okm = [0u8; 32];
    hkdf.expand(info.as_bytes(), &mut okm)
        .map_err(|_| anyhow::anyhow!("HKDF expansion failed"))?;

    // Convert bytes to 5-bit groups for bech32 encoding
    let data_5bit = bech32::convert_bits(&okm, 8, 5, true)
        .map_err(|e| anyhow::anyhow!("bech32 bit conversion failed: {e}"))?;
    let data_u5: Vec<bech32::u5> = data_5bit
        .into_iter()
        .map(bech32::u5::try_from_u8)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("u5 conversion failed: {e}"))?;

    let key_str = bech32::encode("age-secret-key-", &data_u5, bech32::Variant::Bech32)
        .map_err(|e| anyhow::anyhow!("bech32 encode failed: {e}"))?
        .to_uppercase();

    let identity: age::x25519::Identity = key_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse derived age identity: {e}"))?;

    Ok(identity)
}

/// Derive the public key (recipient) corresponding to a derived identity.
pub fn derive_recipient(
    master_key: &[u8],
    owner: &str,
    repo: &str,
    action: &str,
) -> Result<String> {
    let identity = derive_identity(master_key, owner, repo, action)?;
    let recipient = identity.to_public();
    Ok(recipient.to_string())
}

/// Decrypt an age-encrypted ciphertext using a derived identity.
pub fn decrypt(identity: &age::x25519::Identity, ciphertext: &[u8]) -> Result<Vec<u8>> {
    let decryptor = match age::Decryptor::new(ciphertext)
        .map_err(|e| anyhow::anyhow!("Failed to create decryptor: {e}"))?
    {
        age::Decryptor::Recipients(d) => d,
        _ => anyhow::bail!("Unexpected decryptor type"),
    };

    let mut reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|e| anyhow::anyhow!("Decryption failed: {e}"))?;

    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext)?;
    Ok(plaintext)
}
