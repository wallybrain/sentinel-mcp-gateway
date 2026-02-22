use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("token expired")]
    ExpiredToken,
    #[error("invalid claims: {0}")]
    InvalidClaims(String),
    #[error("missing token")]
    MissingToken,
}

impl AuthError {
    pub fn json_rpc_code(&self) -> i64 {
        -32001
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    pub iss: String,
    pub aud: String,
    pub exp: usize,
    pub iat: Option<usize>,
    pub jti: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CallerIdentity {
    pub subject: String,
    pub role: String,
    pub token_id: Option<String>,
}

impl From<Claims> for CallerIdentity {
    fn from(claims: Claims) -> Self {
        Self {
            subject: claims.sub,
            role: claims.role,
            token_id: claims.jti,
        }
    }
}

pub struct JwtValidator {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtValidator {
    pub fn new(secret: &[u8], issuer: &str, audience: &str) -> Self {
        let decoding_key = DecodingKey::from_secret(secret);
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_exp = true;

        Self {
            decoding_key,
            validation,
        }
    }

    pub fn validate(&self, token: &str) -> Result<CallerIdentity, AuthError> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
                jsonwebtoken::errors::ErrorKind::InvalidIssuer
                | jsonwebtoken::errors::ErrorKind::InvalidAudience
                | jsonwebtoken::errors::ErrorKind::InvalidSignature => {
                    AuthError::InvalidToken(e.to_string())
                }
                jsonwebtoken::errors::ErrorKind::MissingRequiredClaim(claim) => {
                    AuthError::InvalidClaims(format!("missing required claim: {claim}"))
                }
                _ => AuthError::InvalidToken(e.to_string()),
            })?;

        let claims = token_data.claims;
        if claims.role.is_empty() {
            return Err(AuthError::InvalidClaims("missing role".to_string()));
        }

        Ok(CallerIdentity::from(claims))
    }
}

pub fn create_token(claims: &Claims, secret: &[u8]) -> Result<String, AuthError> {
    let key = EncodingKey::from_secret(secret);
    encode(&Header::default(), claims, &key)
        .map_err(|e| AuthError::InvalidToken(e.to_string()))
}

pub fn now_secs() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs() as usize
}
