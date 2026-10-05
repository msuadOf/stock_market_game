use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use native_store::{identities::SubjectRecord, NativeDatabase, StoreError};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentitySubject {
    pub subject_id: String,
    pub username: Option<String>,
}

#[derive(Serialize)]
pub struct LoginResult {
    pub subject: IdentitySubject,
    pub token: String,
}

#[derive(Clone)]
pub struct IdentityService {
    database: NativeDatabase,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("用户名应为 1—64 个 ASCII 字母、数字、下划线或短横线；密码应为 8—1024 个 UTF-8 字节")]
    InvalidInput,
    #[error("用户名已注册，请登录或选择其他用户名")]
    UsernameExists,
    #[error("用户名或密码不正确，请重新输入")]
    InvalidLogin,
    #[error("登录凭据缺失或无效，请重新登录")]
    Unauthorized,
    #[error("身份数据库操作失败，请重试并反馈 Server.identity")]
    Storage,
    #[error("密码哈希或系统随机源异常，请反馈 Server.identity")]
    Cryptography,
}

impl IdentityService {
    pub fn new(database: NativeDatabase) -> Self {
        Self { database }
    }

    pub fn register(&self, username: &str, password: &str) -> Result<LoginResult, IdentityError> {
        validate_input(username, password)?;
        let salt =
            SaltString::encode_b64(&random_bytes()?).map_err(|_| IdentityError::Cryptography)?;
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| IdentityError::Cryptography)?
            .to_string();
        self.create_subject(Some(username.to_owned()), Some(password_hash))
    }

    pub fn guest(&self) -> Result<LoginResult, IdentityError> {
        self.create_subject(None, None)
    }

    fn create_subject(
        &self,
        username: Option<String>,
        password_hash: Option<String>,
    ) -> Result<LoginResult, IdentityError> {
        let record = SubjectRecord {
            subject_id: uuid::Uuid::new_v4().to_string(),
            username,
            password_hash,
        };
        let token = hex(&random_bytes()?);
        self.database
            .create_subject_with_credential(&record, &credential_hash(&token))
            .map_err(storage_error)?;
        Ok(LoginResult {
            subject: public_subject(record),
            token,
        })
    }

    pub fn login(&self, username: &str, password: &str) -> Result<LoginResult, IdentityError> {
        validate_input(username, password)?;
        let record = self
            .database
            .subject_by_username(username)
            .map_err(storage_error)?
            .ok_or(IdentityError::InvalidLogin)?;
        let hash = record
            .password_hash
            .as_ref()
            .ok_or(IdentityError::Cryptography)?;
        let parsed = PasswordHash::new(hash).map_err(|_| IdentityError::Cryptography)?;
        match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => {}
            Err(argon2::password_hash::Error::Password) => return Err(IdentityError::InvalidLogin),
            Err(_) => return Err(IdentityError::Cryptography),
        }
        let token = hex(&random_bytes()?);
        self.database
            .create_credential(&record.subject_id, &credential_hash(&token))
            .map_err(storage_error)?;
        Ok(LoginResult {
            subject: public_subject(record),
            token,
        })
    }

    pub fn authenticate(&self, token: &str) -> Result<IdentitySubject, IdentityError> {
        validate_token(token)?;
        let record = self
            .database
            .subject_by_credential_hash(&credential_hash(token))
            .map_err(storage_error)?
            .ok_or(IdentityError::Unauthorized)?;
        Ok(public_subject(record))
    }

    pub fn logout(&self, token: &str) -> Result<(), IdentityError> {
        validate_token(token)?;
        if self
            .database
            .revoke_credential(&credential_hash(token))
            .map_err(storage_error)?
        {
            Ok(())
        } else {
            Err(IdentityError::Unauthorized)
        }
    }
}

fn public_subject(record: SubjectRecord) -> IdentitySubject {
    IdentitySubject {
        subject_id: record.subject_id,
        username: record.username,
    }
}
fn validate_input(username: &str, password: &str) -> Result<(), IdentityError> {
    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, b'_' | b'-'))
        || !(8..=1024).contains(&password.len())
    {
        return Err(IdentityError::InvalidInput);
    }
    Ok(())
}
fn validate_token(token: &str) -> Result<(), IdentityError> {
    if token.len() != 64
        || !token
            .bytes()
            .all(|character| character.is_ascii_digit() || (b'a'..=b'f').contains(&character))
    {
        return Err(IdentityError::Unauthorized);
    }
    Ok(())
}
fn random_bytes() -> Result<[u8; 32], IdentityError> {
    let mut bytes = [0; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| IdentityError::Cryptography)?;
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn credential_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}
fn storage_error(error: StoreError) -> IdentityError {
    match error {
        StoreError::Invalid(message) if message == "IDENTITY_USERNAME_EXISTS" => {
            IdentityError::UsernameExists
        }
        _ => IdentityError::Storage,
    }
}
