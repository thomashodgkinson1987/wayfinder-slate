//! Authentication: user store, session persistence, and module glue.

pub mod users;

/// The axum-login backend: our SQLite-backed user store.
pub type Backend = users::Users;

/// Session-backed authentication state for handlers; populated by the
/// session + auth layers on every request.
pub type AuthSession = axum_login::AuthSession<Backend>;
