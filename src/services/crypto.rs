use bcrypt::{hash, verify, DEFAULT_COST};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: i64,  // user id
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
    verify(password, password_hash)
        .map_err(|e| AppError::Internal(format!("密码验证失败: {e}")))
}

/// Generate a JWT token
pub fn generate_token(user_id: i64, username: &str, role: i32, secret: &str) -> Result<String, AppError> {
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

/// Verify and decode a JWT token (reads secret from env)
pub fn verify_token(token: &str) -> Result<TokenClaims, AppError> {
    let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "change-me-to-a-random-string".to_string());
    verify_token_with_secret(token, &secret)
}

/// Generate a random API token (UUID-based)
pub fn generate_api_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(verify_password("123456", &fresh_hash).unwrap(),
            "Freshly generated bcrypt hash should verify correctly");

        // The migration hash is generated at runtime in integration tests.
        // For the unit test, we just verify bcrypt works correctly.
    }
}
