// 网盘凭据对称加密（feature 047）— AES-256-GCM，主密钥来自 PAN_CRED_KEY（base64 32 字节）
// nonce 每次随机（uuid v4 CSPRNG 取 12 字节），避免引入 rand 依赖（constitution V YAGNI）

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

use crate::errors::AppError;

/// 加密凭据明文 → (cipher_b64, nonce_b64)
pub fn encrypt_credential(plaintext: &str, key_b64: &str) -> Result<(String, String), AppError> {
    let key = decode_key(key_b64)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| AppError::Internal("网盘主密钥长度无效".into()))?;
    let nonce_bytes = random_nonce();
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
        .map_err(|e| AppError::Internal(format!("凭据加密失败: {e}")))?;
    Ok((B64.encode(ct), B64.encode(nonce_bytes)))
}

/// 解密凭据密文 → 明文
pub fn decrypt_credential(
    cipher_b64: &str,
    nonce_b64: &str,
    key_b64: &str,
) -> Result<String, AppError> {
    let key = decode_key(key_b64)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| AppError::Internal("网盘主密钥长度无效".into()))?;
    let ct = B64
        .decode(cipher_b64)
        .map_err(|_| AppError::Internal("凭据密文非合法 base64".into()))?;
    let nonce_bytes = B64
        .decode(nonce_b64)
        .map_err(|_| AppError::Internal("nonce 非合法 base64".into()))?;
    if nonce_bytes.len() != 12 {
        return Err(AppError::Internal("nonce 长度无效".into()));
    }
    let pt = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ct.as_slice())
        .map_err(|_| AppError::Internal("凭据解密失败（密钥错误或数据损坏）".into()))?;
    String::from_utf8(pt).map_err(|_| AppError::Internal("凭据明文非 UTF-8".into()))
}

/// 校验主密钥是否就绪（非空 + 合法 base64 + 32 字节）
pub fn validate_pan_key(key_b64: &str) -> Result<(), AppError> {
    decode_key(key_b64)?;
    Ok(())
}

fn decode_key(key_b64: &str) -> Result<[u8; 32], AppError> {
    if key_b64.trim().is_empty() {
        return Err(AppError::Internal(
            "PAN_CRED_KEY 未配置：请设置 32 字节随机密钥（base64，如 openssl rand -base64 32）"
                .into(),
        ));
    }
    let raw = B64
        .decode(key_b64.trim())
        .map_err(|_| AppError::Internal("PAN_CRED_KEY 非合法 base64".into()))?;
    if raw.len() != 32 {
        return Err(AppError::Internal(format!(
            "PAN_CRED_KEY 需为 32 字节（base64），当前 {} 字节",
            raw.len()
        )));
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&raw);
    Ok(k)
}

fn random_nonce() -> [u8; 12] {
    let uid = uuid::Uuid::new_v4(); // 16 字节 CSPRNG
    let bytes = uid.as_bytes();
    let mut n = [0u8; 12];
    n.copy_from_slice(&bytes[..12]);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> String {
        B64.encode([0x42u8; 32])
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plain = "__puus=abc; __pus=def; __uid=123";
        let (ct, nonce) = encrypt_credential(plain, &key).unwrap();
        assert_ne!(ct, plain);
        let recovered = decrypt_credential(&ct, &nonce, &key).unwrap();
        assert_eq!(recovered, plain);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let key = test_key();
        let other = B64.encode([0x99u8; 32]);
        let (ct, nonce) = encrypt_credential("secret", &key).unwrap();
        assert!(decrypt_credential(&ct, &nonce, &other).is_err());
    }

    #[test]
    fn test_decrypt_tampered_cipher_fails() {
        let key = test_key();
        let (ct, nonce) = encrypt_credential("secret", &key).unwrap();
        // 篡改密文（翻转首字符）
        let tampered = match ct.chars().next() {
            Some('A') => format!("B{}", &ct[1..]),
            Some(c) => {
                let nb = if c == 'Z' { 'Y' } else { 'Z' };
                format!("{nb}{}", &ct[1..])
            }
            None => ct.clone(),
        };
        assert!(decrypt_credential(&tampered, &nonce, &key).is_err());
    }

    #[test]
    fn test_empty_key_rejected() {
        assert!(encrypt_credential("x", "").is_err());
        assert!(validate_pan_key("").is_err());
    }

    #[test]
    fn test_short_key_rejected() {
        let short = B64.encode([0u8; 16]); // 16 字节，非 32
        assert!(encrypt_credential("x", &short).is_err());
        assert!(validate_pan_key(&short).is_err());
    }

    #[test]
    fn test_invalid_base64_key_rejected() {
        assert!(validate_pan_key("!!!not-base64!!!").is_err());
    }

    #[test]
    fn test_validate_good_key_ok() {
        assert!(validate_pan_key(&test_key()).is_ok());
    }
}
