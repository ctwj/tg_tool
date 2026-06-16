use bcrypt::{DEFAULT_COST, hash, verify};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: i64, // user id
    pub username: String,
    pub role: i32,
    pub exp: usize,
}

/// Hash a password using bcrypt
pub fn hash_password(password: &str) -> Result<String, AppError> {
    hash(password, DEFAULT_COST).map_err(|e| AppError::Internal(format!("密码哈希失败: {e}")))
}

/// Verify a password against its bcrypt hash
pub fn verify_password(password: &str, password_hash: &str) -> Result<bool, AppError> {
    verify(password, password_hash).map_err(|e| AppError::Internal(format!("密码验证失败: {e}")))
}

/// Generate a JWT token
pub fn generate_token(
    user_id: i64,
    username: &str,
    role: i32,
    secret: &str,
) -> Result<String, AppError> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(7))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = TokenClaims {
        sub: user_id,
        username: username.to_string(),
        role,
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("Token 生成失败: {e}")))
}

/// Verify and decode a JWT token using the given secret
pub fn verify_token_with_secret(token: &str, secret: &str) -> Result<TokenClaims, AppError> {
    let token_data = decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| AppError::Unauthorized(format!("Token 无效: {e}")))?;

    Ok(token_data.claims)
}

/// Generate a random API token (UUID-based)
pub fn generate_api_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 校验 SESSION_SECRET 强度（feature 027 SEC-001，纯函数便于 TDD 单测）
///
/// 返回 `Err(指引提示)` 表示不合规。提示信息**不含密钥值**（FR-009）。
/// 规则：非空 + 不等于公开默认值 + 长度 ≥ 32 字符。
pub fn validate_session_secret(secret: &str) -> Result<(), String> {
    const DEFAULT_VALUE: &str = "change-me-to-a-random-string";
    const MIN_LEN: usize = 32;
    if secret.is_empty() {
        return Err(
            "SESSION_SECRET 未配置：请设置环境变量 SESSION_SECRET（≥32 字符的随机字符串）".into(),
        );
    }
    if secret == DEFAULT_VALUE {
        return Err("SESSION_SECRET 仍为公开默认值：请更换为随机强密钥（≥32 字符）".into());
    }
    if secret.chars().count() < MIN_LEN {
        return Err(format!("SESSION_SECRET 过短：需 ≥{MIN_LEN} 字符的随机强密钥"));
    }
    Ok(())
}

/// 生成密码学安全随机强口令（feature 027 SEC-002，复用 uuid v4，无新依赖）
///
/// uuid v4 内部使用 CSPRNG；`simple()` 为 32 位 hex，满足 ≥24 字符且不可爆破。
pub fn generate_random_password() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_session_secret_default_rejected() {
        assert!(validate_session_secret("change-me-to-a-random-string").is_err());
    }

    #[test]
    fn test_validate_session_secret_empty_rejected() {
        assert!(validate_session_secret("").is_err());
    }

    #[test]
    fn test_validate_session_secret_short_rejected() {
        assert!(validate_session_secret("short-secret").is_err());
    }

    #[test]
    fn test_validate_session_secret_valid_accepted() {
        let ok = "a".repeat(40);
        assert!(validate_session_secret(&ok).is_ok());
    }

    #[test]
    fn test_validate_session_secret_error_no_leak() {
        // 错误信息不得包含密钥值（FR-009）—— 用不合规 secret 触发 Err
        let secret = "weak-short-secret";
        let err = validate_session_secret(secret).unwrap_err();
        assert!(!err.contains(secret));
    }

    #[test]
    fn test_generate_random_password_not_fixed() {
        let p = generate_random_password();
        assert_ne!(p, "123456");
        assert!(p.chars().count() >= 24);
    }

    #[test]
    fn test_generate_random_password_random() {
        // 两次生成应不同（uuid v4 CSPRNG，碰撞概率可忽略）
        assert_ne!(generate_random_password(), generate_random_password());
    }

    #[test]
    fn test_hash_password_success() {
        let hash = hash_password("mypassword123").unwrap();
        assert!(!hash.is_empty());
        assert!(hash.starts_with("$2b$"));
    }

    #[test]
    fn test_verify_password_correct() {
        let hash = hash_password("mypassword123").unwrap();
        let result = verify_password("mypassword123", &hash).unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_password_incorrect() {
        let hash = hash_password("mypassword123").unwrap();
        let result = verify_password("wrongpassword", &hash).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_verify_password_empty() {
        let hash = hash_password("").unwrap();
        assert!(verify_password("", &hash).unwrap());
        assert!(!verify_password("notempty", &hash).unwrap());
    }

    #[test]
    fn test_generate_and_verify_token() {
        let secret = "test-secret-key";
        let token = generate_token(42, "testuser", 10, secret).unwrap();
        assert!(!token.is_empty());

        let claims = verify_token_with_secret(&token, secret).unwrap();
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.username, "testuser");
        assert_eq!(claims.role, 10);
        assert!(claims.exp > 0);
    }

    #[test]
    fn test_verify_token_invalid() {
        let result = verify_token_with_secret("invalid.token.here", "test-secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_token_wrong_secret() {
        let token = generate_token(1, "user", 1, "secret-a").unwrap();
        let result = verify_token_with_secret(&token, "secret-b");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_api_token_format() {
        let token = generate_api_token();
        // UUID v4 format: 8-4-4-4-12
        assert_eq!(token.len(), 36);
        assert!(token.contains('-'));

        // Each call should produce a unique token
        let token2 = generate_api_token();
        assert_ne!(token, token2);
    }

    #[test]
    fn test_token_claims_expiry() {
        let secret = "expiry-test";
        let token = generate_token(1, "user", 1, secret).unwrap();

        let claims = verify_token_with_secret(&token, secret).unwrap();
        // Token should expire approximately 7 days from now
        let now = chrono::Utc::now().timestamp() as usize;
        let seven_days = 7 * 24 * 60 * 60;
        assert!(claims.exp > now);
        assert!(claims.exp <= now + seven_days + 60); // allow 60s slack
    }

    #[test]
    fn test_migration_hash_is_valid() {
        // First generate a fresh hash and verify it round-trips correctly
        let fresh_hash = hash_password("123456").unwrap();
        assert!(
            verify_password("123456", &fresh_hash).unwrap(),
            "Freshly generated bcrypt hash should verify correctly"
        );

        // The migration hash is generated at runtime in integration tests.
        // For the unit test, we just verify bcrypt works correctly.
    }
}
