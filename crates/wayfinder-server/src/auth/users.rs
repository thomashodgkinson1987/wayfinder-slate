//! Argon2id password hashing, user/profile storage, and the axum-login
//! authentication backend.
//!
//! All Argon2 work runs in `tokio::task::spawn_blocking`; ordinary queries
//! stay on the async executor (CONTRIBUTING rule). Failure paths never log
//! password material or PHC hashes.

use std::fmt;

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{Error as PasswordHashError, PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use axum_login::{AuthUser, AuthnBackend, UserId};
use serde::Serialize;
use sqlx::{AssertSqlSafe, Row, Sqlite, SqlitePool, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

/// OWASP-recommended Argon2id baseline: 19 MiB of memory, 2 iterations,
/// 1 lane. Deliberately explicit so crate defaults cannot silently weaken
/// it (plan §2.3).
const ARGON2_MEMORY_KIB: u32 = 19_456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

fn argon2() -> Argon2<'static> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        None,
    )
    .expect("hard-coded Argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a password to a PHC string. Runs off the async runtime.
///
/// Errors occur only for invalid hash configuration or an overlong password
/// (max 512 bytes); callers validate length first.
async fn hash_password(password: &str) -> Result<String, PasswordHashError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        argon2()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await
    .expect("spawn_blocking panic while hashing password")
}

/// Verify a password against a stored PHC string. Runs off the async runtime.
/// An unparsable stored hash (e.g. a tampered database row) counts as a
/// failed verification, not a panic; `MAX_PWD_LEN` only applies at hashing
/// time, so parsing never overflows our 128-character password limit.
async fn verify_password(password: &str, stored: &str) -> bool {
    let password = password.to_owned();
    let stored = stored.to_owned();
    tokio::task::spawn_blocking(move || match PasswordHash::new(&stored) {
        Ok(parsed) => argon2()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    })
    .await
    .expect("spawn_blocking panic while verifying password")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    /// Strict parse of the `role` column; the schema CHECK limits values to
    /// the two enum variants.
    fn parse(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            "user" => Self::User,
            other => panic!("unknown role {other:?}; schema CHECK violated"),
        }
    }
}

#[derive(Clone)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub role: Role,
    pub created_at: i64,
}

impl fmt::Debug for User {
    // Never expose password material through Debug output.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("display_name", &self.display_name)
            .field("role", &self.role)
            .field("created_at", &self.created_at)
            .field("password_hash", &"...")
            .finish()
    }
}

impl AuthUser for User {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }

    // Reuse the PHC string as the session auth hash: changing the password
    // (M4+) changes the stored hash and thereby invalidates every session.
    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_bytes()
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for User {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        let role_text: String = row.try_get("role")?;
        Ok(User {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            password_hash: row.try_get("password_hash")?,
            display_name: row.try_get("display_name")?,
            role: Role::parse(&role_text),
            created_at: row.try_get("created_at")?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GameMode {
    Normal,
    Master,
}

impl GameMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Master => "master",
        }
    }

    /// Parse a client-supplied game mode. `None` for anything unrecognised so
    /// handlers can answer `400` instead of silently defaulting.
    pub fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "master" => Some(Self::Master),
            _ => None,
        }
    }

    /// Strict parse of the `game_mode` column.
    fn parse(value: &str) -> Self {
        match value {
            "normal" => Self::Normal,
            "master" => Self::Master,
            other => panic!("unknown game mode {other:?}; schema CHECK violated"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub game_mode: GameMode,
    pub is_public: bool,
    pub share_global: bool,
    pub created_at: i64,
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for Profile {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        let game_mode_text: String = row.try_get("game_mode")?;
        let is_public: i64 = row.try_get("is_public")?;
        let share_global: i64 = row.try_get("share_global")?;
        Ok(Profile {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            name: row.try_get("name")?,
            game_mode: GameMode::parse(&game_mode_text),
            is_public: is_public != 0,
            share_global: share_global != 0,
            created_at: row.try_get("created_at")?,
        })
    }
}

const USERS_COLUMNS: &str = "id, username, password_hash, display_name, role, created_at";

const PROFILE_COLUMNS: &str = "id, user_id, name, game_mode, is_public, share_global, created_at";

/// Statement assembly: SQL strings are built from `&'static str` module
/// constants only. Nothing user-controlled is ever interpolated; all runtime
/// values travel through bind parameters. `AssertSqlSafe` is sqlx 0.9's
/// mechanism for declaring an owned `String` audited before execution.
fn sql(static_statement: String) -> AssertSqlSafe<String> {
    AssertSqlSafe(static_statement)
}

impl From<sqlx::Error> for CreateUserError {
    fn from(error: sqlx::Error) -> Self {
        Self::Backend(error)
    }
}

/// Credentials accepted by [`AuthnBackend::authenticate`].
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"...")
            .finish()
    }
}

/// Pool-backed user store and authentication backend.
#[derive(Debug, Clone)]
pub struct Users(SqlitePool);

impl Users {
    pub fn new(pool: SqlitePool) -> Self {
        Self(pool)
    }

    async fn by_id(&self, id: &str) -> sqlx::Result<Option<User>> {
        sqlx::query_as(sql(format!(
            "SELECT {USERS_COLUMNS} FROM users WHERE id = ?"
        )))
        .bind(id)
        .fetch_optional(&self.0)
        .await
    }

    async fn by_username(&self, username: &str) -> sqlx::Result<Option<User>> {
        sqlx::query_as(sql(format!(
            "SELECT {USERS_COLUMNS} FROM users WHERE LOWER(username) = LOWER(?)"
        )))
        .bind(username.trim())
        .fetch_optional(&self.0)
        .await
    }

    /// Register a new user: validate, hash, insert with first-user role
    /// assignment, and create the auto-named `Default` profile. All database
    /// steps run in one immediate transaction so concurrent registrations
    /// cannot both obtain the first-user admin role.
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        display_name: Option<&str>,
    ) -> Result<User, CreateUserError> {
        let username = username.trim();
        let display_name = display_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(username);

        validate_username(username).map_err(CreateUserError::Invalid)?;
        validate_password(password).map_err(CreateUserError::Invalid)?;
        validate_display_name(display_name).map_err(CreateUserError::Invalid)?;

        let password_hash = hash_password(password).await.map_err(|error| {
            tracing::debug!(%error, "argon2 hashing failed during registration");
            CreateUserError::Backend(sqlx::Error::Configuration(error.into()))
        })?;

        let id = Uuid::now_v7().to_string();
        let created_at = OffsetDateTime::now_utc().unix_timestamp();

        let mut tx = self
            .0
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(CreateUserError::Backend)?;

        let existing: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE LOWER(username) = LOWER(?)")
                .bind(username)
                .fetch_one(&mut *tx)
                .await
                .map_err(CreateUserError::Backend)?;
        if existing > 0 {
            return Err(CreateUserError::Duplicate);
        }

        // The first registered user on a fresh deployment is its admin.
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&mut *tx)
            .await
            .map_err(CreateUserError::Backend)?;
        let role = if users == 0 { Role::Admin } else { Role::User };

        sqlx::query(sql(format!(
            "INSERT INTO users ({USERS_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?)"
        )))
        .bind(&id)
        .bind(username)
        .bind(&password_hash)
        .bind(display_name)
        .bind(role.as_str())
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(CreateUserError::Backend)?;

        create_default_profile(&mut tx, &id, created_at).await?;

        tx.commit().await.map_err(CreateUserError::Backend)?;

        Ok(User {
            id,
            username: username.to_owned(),
            password_hash,
            display_name: display_name.to_owned(),
            role,
            created_at,
        })
    }

    /// List the calling user's profiles in creation order.
    pub async fn profiles_for_user(&self, user_id: &str) -> sqlx::Result<Vec<Profile>> {
        sqlx::query_as(sql(format!(
            "SELECT {PROFILE_COLUMNS} FROM profiles WHERE user_id = ? ORDER BY created_at, id"
        )))
        .bind(user_id)
        .fetch_all(&self.0)
        .await
    }

    /// Create a new profile for a user.
    pub async fn create_profile(
        &self,
        user_id: &str,
        name: &str,
        game_mode: GameMode,
    ) -> Result<Profile, CreateProfileError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CreateProfileError::Invalid(
                "profile name must not be empty".into(),
            ));
        }
        if name.chars().count() > 48 {
            return Err(CreateProfileError::Invalid(
                "profile name must be at most 48 characters".into(),
            ));
        }

        let id = Uuid::now_v7().to_string();
        let created_at = OffsetDateTime::now_utc().unix_timestamp();

        sqlx::query(sql(format!(
            "INSERT INTO profiles ({PROFILE_COLUMNS}) VALUES (?, ?, ?, ?, 0, 0, ?)"
        )))
        .bind(&id)
        .bind(user_id)
        .bind(name)
        .bind(game_mode.as_str())
        .bind(created_at)
        .execute(&self.0)
        .await
        .map_err(CreateProfileError::Backend)?;

        Ok(Profile {
            id,
            user_id: user_id.to_owned(),
            name: name.to_owned(),
            game_mode,
            is_public: false,
            share_global: false,
            created_at,
        })
    }

    /// Count all registered users. Used for the startup log line (no PII).
    pub(crate) async fn count(&self) -> sqlx::Result<i64> {
        sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.0)
            .await
    }
}

async fn create_default_profile(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    created_at: i64,
) -> sqlx::Result<()> {
    let id = Uuid::now_v7().to_string();
    sqlx::query(sql(format!(
        "INSERT INTO profiles ({PROFILE_COLUMNS}) VALUES (?, ?, 'Default', 'normal', 0, 0, ?)"
    )))
    .bind(&id)
    .bind(user_id)
    .bind(created_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Failures of [`Users::create_user`] mapped to HTTP semantics by handlers:
/// `Invalid` → 400, `Duplicate` → 409, `Backend` → 500.
#[derive(Debug)]
pub enum CreateUserError {
    Invalid(String),
    Duplicate,
    Backend(sqlx::Error),
}

/// Failures of [`Users::create_profile`]: `Invalid` → 400, `Backend` → 500.
#[derive(Debug)]
pub enum CreateProfileError {
    Invalid(String),
    Backend(sqlx::Error),
}

impl AuthnBackend for Users {
    type User = User;
    type Credentials = Credentials;
    type Error = sqlx::Error;

    /// Verify credentials. Unknown users burn an equivalent Argon2
    /// computation so response timing does not reveal which usernames exist.
    /// Login failures stay generic at the HTTP layer.
    async fn authenticate(
        &self,
        credentials: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let user = self.by_username(&credentials.username).await?;
        match user {
            Some(user) => {
                let verified = verify_password(&credentials.password, &user.password_hash).await;
                Ok(verified.then_some(user))
            }
            None => {
                // Unknown username: still pay roughly one hash computation.
                let _ = hash_password(&credentials.password).await;
                Ok(None)
            }
        }
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        self.by_id(user_id).await
    }
}

/// Username rules: 3–24 ASCII word characters (`A–Z a–z 0–9 _ -`).
pub fn validate_username(username: &str) -> Result<(), String> {
    let length = username.chars().count();
    if !(3..=24).contains(&length) {
        return Err(format!("username must be 3-24 characters (got {length})"));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("username may only contain letters, digits, '_' and '-'".into());
    }
    Ok(())
}

/// Password rules: 8–128 characters.
pub fn validate_password(password: &str) -> Result<(), String> {
    let length = password.chars().count();
    if !(8..=128).contains(&length) {
        return Err(format!("password must be 8-128 characters (got {length})"));
    }
    Ok(())
}

/// Display name rules: optional, 1–48 characters after trimming; defaults to
/// the username.
pub fn validate_display_name(display_name: &str) -> Result<(), String> {
    let length = display_name.chars().count();
    if !(1..=48).contains(&length) {
        return Err("display name must be 1-48 characters".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::temp_db;

    async fn users_store() -> (tempfile::TempDir, Users) {
        let (dir, pool) = temp_db().await;
        (dir, Users::new(pool))
    }

    #[test]
    fn argon2_uses_valid_explicit_parameters() {
        // Params::new validates the constants; zero or over-large values
        // would panic here.
        let _ = argon2();
    }

    #[tokio::test]
    async fn hash_and_verify_round_trip() {
        let hash = hash_password("hunter2secret").await.unwrap();
        assert!(!hash.contains("hunter2secret"));
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("hunter2secret", &hash).await);
        assert!(!verify_password("wrong", &hash).await);
        // A garbage stored hash must simply fail verification.
        assert!(!verify_password("hunter2secret", "$argon2id$garbage").await);
    }

    #[tokio::test]
    async fn first_user_becomes_admin_and_gets_a_default_profile() {
        let (dir, users) = users_store().await;

        let first = users
            .create_user("Zeta", "correct horse", None)
            .await
            .unwrap();
        assert_eq!(first.role, Role::Admin);
        assert_eq!(first.display_name, "Zeta");
        assert_eq!(first.username, "Zeta"); // stored as given

        let second = users
            .create_user("other-user", "hunter2hunter", Some("Yoshi"))
            .await
            .unwrap();
        assert_eq!(second.role, Role::User);
        assert_eq!(second.display_name, "Yoshi");

        let profiles = users.profiles_for_user(&first.id).await.unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "Default");
        assert_eq!(profiles[0].game_mode, GameMode::Normal);
        assert!(!profiles[0].is_public);
        assert!(!profiles[0].share_global);

        drop(users);
        drop(dir);
    }

    #[tokio::test]
    async fn duplicate_username_is_rejected_case_insensitively() {
        let (dir, users) = users_store().await;

        users
            .create_user("Kite", "longpassword1", None)
            .await
            .unwrap();
        let error = users
            .create_user("kite", "longpassword1", None)
            .await
            .unwrap_err();
        assert!(matches!(error, CreateUserError::Duplicate), "{error:?}");

        let error = users
            .create_user("  KITE  ", "longpassword1", None)
            .await
            .unwrap_err();
        assert!(matches!(error, CreateUserError::Duplicate), "{error:?}");

        drop(users);
        drop(dir);
    }

    #[tokio::test]
    async fn invalid_registration_payloads_are_rejected() {
        let (dir, users) = users_store().await;

        for case in [
            ("ab", "longpassword1", None), // username too short
            ("toolongusername-willnotfit24", "longpassword1", None), // username too long
            ("bad name", "longpassword1", None), // space
            ("okuser", "short", None),     // password too short
            ("okuser", "", None),          // password empty
            ("$pecial", "longpassword1", None), // bad characters
        ] {
            let error = users.create_user(case.0, case.1, case.2).await.unwrap_err();
            assert!(
                matches!(error, CreateUserError::Invalid(_)),
                "expected Invalid for {case:?}: {error:?}"
            );
        }

        drop(users);
        drop(dir);
    }

    #[tokio::test]
    async fn empty_display_name_falls_back_to_username() {
        let (dir, users) = users_store().await;

        let user = users
            .create_user("fallback", "longpassword1", Some("   "))
            .await
            .unwrap();
        assert_eq!(user.display_name, "fallback");

        drop(users);
        drop(dir);
    }

    #[tokio::test]
    async fn backend_authenticates_only_with_the_right_password() {
        let (dir, users) = users_store().await;

        let registered = users
            .create_user("Watcher", "longpassword1", None)
            .await
            .unwrap();

        let found = users
            .authenticate(Credentials {
                username: "watcher".into(), // case-insensitive lookup
                password: "longpassword1".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            found.as_ref().map(|u| u.id.clone()),
            Some(registered.id.clone())
        );

        let wrong = users
            .authenticate(Credentials {
                username: "Watcher".into(),
                password: "longpassword2".into(),
            })
            .await
            .unwrap();
        assert!(wrong.is_none());

        let unknown = users
            .authenticate(Credentials {
                username: "nobody".into(),
                password: "longpassword1".into(),
            })
            .await
            .unwrap();
        assert!(unknown.is_none());

        // get_user round trip through the trait method.
        let fetched = users.get_user(&registered.id).await.unwrap();
        assert!(fetched.is_some());

        drop(users);
        drop(dir);
    }

    #[tokio::test]
    async fn profiles_validate_and_default_to_normal_mode() {
        let (dir, users) = users_store().await;

        let user = users
            .create_user("ProfileUser", "longpassword1", None)
            .await
            .unwrap();

        let created = users
            .create_profile(&user.id, "  Master run ", GameMode::Master)
            .await
            .unwrap();
        assert_eq!(created.name, "Master run");
        assert_eq!(created.game_mode, GameMode::Master);

        let error = users
            .create_profile(&user.id, "   ", GameMode::Normal)
            .await
            .unwrap_err();
        assert!(matches!(error, CreateProfileError::Invalid(_)), "{error:?}");

        let listed = users.profiles_for_user(&user.id).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Default");

        drop(users);
        drop(dir);
    }

    #[test]
    fn validation_rules_match_the_plan() {
        for name in ["abc", "ABC", "user_1", "a-b-c", "x".repeat(24).as_str()] {
            assert!(validate_username(name).is_ok(), "{name:?}");
        }
        for name in [
            "",
            "ab",
            "ab cd",
            "ab.cd",
            "ab!cd",
            "日本",
            "x".repeat(25).as_str(),
        ] {
            assert!(validate_username(name).is_err(), "{name:?}");
        }
        assert!(validate_display_name("y".repeat(48).as_str()).is_ok());
        assert!(validate_display_name("y".repeat(49).as_str()).is_err());
    }
}
