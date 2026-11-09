use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_login::AuthManagerLayerBuilder;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};

use crate::auth::session_store::SqlxSessionStore;
use crate::auth::users::{CreateProfileError, CreateUserError, Credentials, GameMode, User};
use crate::auth::{AuthSession, Backend};
use crate::config::{Config, Mode, SESSION_TTL};
use crate::saves;

/// Shared router state: configuration, the SQLite pool, and the auth backend.
/// Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: SqlitePool,
    pub backend: Backend,
}

#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
struct Assets;

#[derive(Serialize)]
struct Health {
    ok: bool,
    version: &'static str,
}

#[derive(Serialize)]
struct PublicConfig {
    mode: crate::config::Mode,
    version: &'static str,
    live: bool,
    save_dir: Option<String>,
    control_addr: Option<String>,
    map_assets: bool,
    /// True when save/profile data requires a signed-in user (LAN/hub).
    auth_required: bool,
    /// Open registration is enabled for v1; M8 may gate it per mode.
    registration_open: bool,
}

#[derive(Deserialize)]
struct CompletionQuery {
    slot: Option<usize>,
}

/// Public shape of a user in API responses. Never includes `password_hash`.
#[derive(Serialize)]
struct PublicUser {
    id: String,
    username: String,
    display_name: String,
    role: crate::auth::users::Role,
    created_at: i64,
}

impl From<&User> for PublicUser {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.clone(),
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            role: user.role,
            created_at: user.created_at,
        }
    }
}

#[derive(Serialize)]
struct UserEnvelope {
    user: PublicUser,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CreateProfileRequest {
    name: String,
    #[serde(default)]
    game_mode: Option<String>,
}

pub fn router(state: AppState) -> Router {
    // The AuthManagerLayer provides both the session middleware and the auth
    // session extension, so it must be the outermost of the two.
    let session_layer = SessionManagerLayer::new(SqlxSessionStore::new(state.db.clone()))
        .with_secure(state.config.cookie_secure)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(SESSION_TTL));
    let auth_layer = AuthManagerLayerBuilder::new(state.backend.clone(), session_layer).build();

    // Save data is open on the local machine but requires a login on LAN/hub.
    let save_routes = Router::new()
        .route("/api/slots", get(list_slots))
        .route("/api/slots/{slot}", get(slot_detail))
        .route("/api/slots/{slot}/caption", get(slot_caption))
        .route("/api/slots/{slot}/map", get(slot_map))
        .route("/api/completion", get(completion))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth_outside_local,
        ));

    // Per-user resources require a signed-in user in every mode.
    let account_routes = Router::new()
        .route("/api/me", get(me))
        .route("/api/profiles", get(list_profiles).post(create_profile))
        .route_layer(middleware::from_fn(require_auth_always));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(public_config))
        .route("/api/map-objects", get(map_objects))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .merge(save_routes)
        .merge(account_routes)
        .fallback(static_asset)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(auth_layer)
        .with_state(state)
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn public_config(State(state): State<AppState>) -> Json<PublicConfig> {
    let config = &*state.config;
    Json(PublicConfig {
        mode: config.mode,
        version: env!("CARGO_PKG_VERSION"),
        live: config.mode.supports_direct_live(),
        save_dir: config.save_dir_for_display(),
        control_addr: config
            .mode
            .supports_direct_live()
            .then(|| config.control_addr.to_string()),
        map_assets: Assets::get("map/botw/tiles/0/0_0.jpg").is_some(),
        auth_required: config.mode != Mode::Local,
        registration_open: true,
    })
}

/// Is the request's auth session attached to a signed-in user?
fn is_authenticated(request: &Request) -> bool {
    request
        .extensions()
        .get::<AuthSession>()
        .is_some_and(|session| session.user.is_some())
}

/// Policy guard for save-data routes: no auth required on the local machine,
/// JSON `401` on LAN/hub.
async fn require_auth_outside_local(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.config.mode == Mode::Local || is_authenticated(&request) {
        next.run(request).await
    } else {
        json_error(StatusCode::UNAUTHORIZED, "authentication required")
    }
}

/// Policy guard for per-user resources: always require a signed-in user.
async fn require_auth_always(request: Request, next: Next) -> Response {
    if is_authenticated(&request) {
        next.run(request).await
    } else {
        json_error(StatusCode::UNAUTHORIZED, "authentication required")
    }
}

async fn map_objects() -> Response {
    match Assets::get("data/map_objects.json") {
        Some(asset) => asset_response("data/map_objects.json", asset.data.into_owned()),
        None => (StatusCode::NOT_FOUND, "map objects dataset not found").into_response(),
    }
}

async fn list_slots(State(state): State<AppState>) -> Response {
    let slots = saves::list_slots(&state.config, map_dataset());
    no_store(Json(slots).into_response())
}

async fn slot_detail(State(state): State<AppState>, Path(slot): Path<usize>) -> Response {
    match saves::load_slot(&state.config, slot, map_dataset()) {
        Ok(detail) => no_store(Json(detail).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn slot_map(State(state): State<AppState>, Path(slot): Path<usize>) -> Response {
    match saves::load_map_completion(&state.config, slot, map_dataset()) {
        Ok(completion) => no_store(Json(completion).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn slot_caption(State(state): State<AppState>, Path(slot): Path<usize>) -> Response {
    match saves::load_caption(&state.config, slot) {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
            no_store(response)
        }
        Err(error) => slot_error_response(slot, error),
    }
}

async fn completion(
    State(state): State<AppState>,
    Query(query): Query<CompletionQuery>,
) -> Response {
    let slot = query.slot.unwrap_or(0);
    match saves::load_completion(&state.config, slot) {
        Ok(summary) => no_store(Json(summary).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

fn slot_error_response(slot: usize, error: saves::SlotError) -> Response {
    match error {
        saves::SlotError::SlotNotFound => {
            (StatusCode::NOT_FOUND, "save slot not found").into_response()
        }
        saves::SlotError::CaptionNotFound => {
            (StatusCode::NOT_FOUND, "caption not found").into_response()
        }
        saves::SlotError::Unreadable(error) => {
            tracing::warn!(slot, %error, "could not read save for slot");
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "save file could not be parsed",
            )
                .into_response()
        }
    }
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn map_dataset() -> &'static saves::MapObjects {
    static OBJECTS: OnceLock<saves::MapObjects> = OnceLock::new();

    OBJECTS.get_or_init(|| match Assets::get("data/map_objects.json") {
        Some(asset) => match saves::MapObjects::from_json(&asset.data) {
            Ok(objects) => objects,
            Err(error) => {
                tracing::warn!(%error, "could not parse map objects for slot labels");
                saves::MapObjects::default()
            }
        },
        None => saves::MapObjects::default(),
    })
}

/// Register a new account, create its default profile, and sign it in.
async fn register(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    Json(payload): Json<RegisterRequest>,
) -> Response {
    match state
        .backend
        .create_user(
            &payload.username,
            &payload.password,
            payload.display_name.as_deref(),
        )
        .await
    {
        Ok(user) => {
            if let Err(error) = auth_session.login(&user).await {
                tracing::error!(%error, "could not start a session after registration");
                return internal_error();
            }
            (
                StatusCode::CREATED,
                Json(UserEnvelope {
                    user: PublicUser::from(&user),
                }),
            )
                .into_response()
        }
        Err(CreateUserError::Invalid(message)) => json_error(StatusCode::BAD_REQUEST, message),
        Err(CreateUserError::Duplicate) => {
            json_error(StatusCode::CONFLICT, "username is already taken")
        }
        Err(CreateUserError::Backend(error)) => {
            tracing::error!(%error, "registration failed");
            internal_error()
        }
    }
}

/// Authenticate and start a session. Every failure mode returns the same
/// generic message so account existence is never disclosed.
async fn login(mut auth_session: AuthSession, Json(payload): Json<LoginRequest>) -> Response {
    let credentials = Credentials {
        username: payload.username,
        password: payload.password,
    };

    match auth_session.authenticate(credentials).await {
        Ok(Some(user)) => {
            if let Err(error) = auth_session.login(&user).await {
                tracing::error!(%error, "could not start a session after login");
                return internal_error();
            }
            (
                StatusCode::OK,
                Json(UserEnvelope {
                    user: PublicUser::from(&user),
                }),
            )
                .into_response()
        }
        Ok(None) => json_error(StatusCode::UNAUTHORIZED, "invalid credentials"),
        Err(error) => {
            tracing::error!(%error, "login failed");
            internal_error()
        }
    }
}

/// End the session. Idempotent: returns `204` even when not signed in.
async fn logout(mut auth_session: AuthSession) -> Response {
    match auth_session.logout().await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(%error, "logout failed");
            internal_error()
        }
    }
}

async fn me(auth_session: AuthSession) -> Response {
    match auth_session.user {
        Some(user) => Json(UserEnvelope {
            user: PublicUser::from(&user),
        })
        .into_response(),
        None => json_error(StatusCode::UNAUTHORIZED, "authentication required"),
    }
}

async fn list_profiles(auth_session: AuthSession) -> Response {
    let Some(user) = auth_session.user else {
        return json_error(StatusCode::UNAUTHORIZED, "authentication required");
    };

    match auth_session.backend.profiles_for_user(&user.id).await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(error) => {
            tracing::error!(%error, "could not list profiles");
            internal_error()
        }
    }
}

async fn create_profile(
    auth_session: AuthSession,
    Json(payload): Json<CreateProfileRequest>,
) -> Response {
    let Some(user) = auth_session.user else {
        return json_error(StatusCode::UNAUTHORIZED, "authentication required");
    };

    let game_mode = match payload.game_mode.as_deref() {
        None => GameMode::Normal,
        Some(value) => match GameMode::from_name(value) {
            Some(mode) => mode,
            None => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "game_mode must be either \"normal\" or \"master\"",
                );
            }
        },
    };

    match auth_session
        .backend
        .create_profile(&user.id, &payload.name, game_mode)
        .await
    {
        Ok(profile) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(CreateProfileError::Invalid(message)) => json_error(StatusCode::BAD_REQUEST, message),
        Err(CreateProfileError::Backend(error)) => {
            tracing::error!(%error, "could not create profile");
            internal_error()
        }
    }
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
        .into_response()
}

fn internal_error() -> Response {
    json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
}

async fn static_asset(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let requested = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };

    if let Some(asset) = Assets::get(requested) {
        return asset_response(requested, asset.data.into_owned());
    }

    // Unknown paths without an extension are SPA routes; everything else is a
    // genuinely missing asset and should not be answered with HTML.
    if !requested.rsplit('/').next().unwrap_or("").contains('.') {
        if let Some(index) = Assets::get("index.html") {
            return asset_response("index.html", index.data.into_owned());
        }
    }

    if Assets::get("index.html").is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Wayfinder Slate UI is not built. Run `npm --prefix frontend run build`.",
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.starts_with("assets/") || path.starts_with("map/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, mime.to_string().parse().unwrap());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, cache.parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::to_bytes;

    use super::*;
    use crate::config::Mode;
    use crate::saves::test_support::{caption_save, temp_root, valid_save, write_slot};

    async fn test_state(mode: Mode, save_dir: Option<PathBuf>) -> (tempfile::TempDir, AppState) {
        let (db_dir, db) = crate::db::temp_db().await;
        let config = Arc::new(Config {
            mode,
            bind: "127.0.0.1:0".parse().unwrap(),
            save_dir,
            control_addr: "127.0.0.1:7777".parse().unwrap(),
            data_dir: db_dir.path().to_path_buf(),
            cookie_secure: false,
        });
        let backend = Backend::new(db.clone());
        (
            db_dir,
            AppState {
                config,
                db,
                backend,
            },
        )
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    #[tokio::test]
    async fn slot_endpoints_serve_data_and_map_errors() {
        let root = temp_root("http");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            Some(b"jpeg-bytes"),
        );
        write_slot(&root.join("save_0"), 1, b"not a save", None, None);
        write_slot(
            &root.join("save_0"),
            2,
            &valid_save(),
            Some(&caption_save()),
            None,
        );

        let (db_dir, state) = test_state(Mode::Local, Some(root.clone())).await;

        let response = list_slots(State(state.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let slots: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert_eq!(slots.as_array().unwrap().len(), 3);

        let response = slot_detail(State(state.clone()), Path(0)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let detail: serde_json::Value =
            serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert_eq!(detail["slot"].as_u64(), Some(0));
        assert_eq!(detail["stats"]["play_time_seconds"].as_u64(), Some(50_166));
        assert_eq!(detail["stats"]["hearts"]["current"].as_f64(), Some(4.0));
        assert_eq!(detail["stats"]["location"]["map"].as_str(), Some("G-6"));
        assert_eq!(
            detail["completion"]["shrines"]["completed"].as_u64(),
            Some(1)
        );

        let response = slot_detail(State(state.clone()), Path(9)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_detail(State(state.clone()), Path(1)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let response = slot_map(State(state.clone()), Path(0)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let map: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert_eq!(map["slot"].as_u64(), Some(0));
        for key in [
            "shrines",
            "towers",
            "memories",
            "koroks",
            "locations",
            "enemies",
            "divine_beasts",
            "treasures",
        ] {
            assert!(map["completed"][key].is_array(), "{key} should be an array");
        }

        let response = slot_map(State(state.clone()), Path(9)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_map(State(state.clone()), Path(1)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let response = slot_caption(State(state.clone()), Path(0)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(body_bytes(response).await, b"jpeg-bytes".to_vec());

        let response = slot_caption(State(state.clone()), Path(2)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_caption(State(state), Path(9)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(root).unwrap();
        drop(db_dir);
    }

    #[tokio::test]
    async fn hub_mode_returns_no_slots() {
        let root = temp_root("http-hub");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            Some(b"jpeg-bytes"),
        );
        let (db_dir, state) = test_state(Mode::Hub, Some(root.clone())).await;

        let response = list_slots(State(state.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, b"[]".to_vec());

        let response = slot_detail(State(state.clone()), Path(0)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_caption(State(state), Path(0)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(root).unwrap();
        drop(db_dir);
    }

    // --- auth, policy, and profile integration tests ---------------------

    use axum::http::Method;
    use serde_json::json;
    use tower::ServiceExt;

    /// Drive the full router (all middleware applied) through one request,
    /// returning the status, first `Set-Cookie` pair (name=value only), and
    /// parsed JSON body.
    async fn send(
        app: &Router,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
        cookie: Option<&str>,
    ) -> (StatusCode, Option<String>, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        let request = builder
            .body(Body::from(
                body.map(|value| value.to_string()).unwrap_or_default(),
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or_default().to_string());

        let bytes = body_bytes(response).await;
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, cookie, json)
    }

    async fn register(
        app: &Router,
        username: &str,
        password: &str,
    ) -> (StatusCode, Option<String>, serde_json::Value) {
        send(
            app,
            Method::POST,
            "/api/auth/register",
            Some(json!({ "username": username, "password": password })),
            None,
        )
        .await
    }

    #[tokio::test]
    async fn health_and_config_stay_public_and_show_policy_flags() {
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);

        let (status, _, body) = send(&app, Method::GET, "/api/health", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);

        let (status, _, body) = send(&app, Method::GET, "/api/config", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["auth_required"], false, "local mode is open");
        assert_eq!(body["registration_open"], true);

        let (db_dir2, state2) = test_state(Mode::Hub, None).await;
        let app2 = router(state2);
        let (status, _, body) = send(&app2, Method::GET, "/api/config", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["auth_required"], true, "hub requires auth");

        drop(db_dir);
        drop(db_dir2);
    }

    #[tokio::test]
    async fn register_login_logout_lifecycle() {
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);

        let (status, cookie, body) = register(&app, "Thistle", "longpassword1").await;
        assert_eq!(status, StatusCode::CREATED);
        let cookie = cookie.expect("registration should start a session");
        assert_eq!(body["user"]["username"], "Thistle");
        assert_eq!(body["user"]["role"], "admin", "first user is admin");
        assert!(body["user"].get("password_hash").is_none());

        let (status, _, body) = send(&app, Method::GET, "/api/me", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["user"]["username"], "Thistle");

        let (status, _, body) = send(&app, Method::GET, "/api/me", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "authentication required");

        let (status, _, _) =
            send(&app, Method::POST, "/api/auth/logout", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // The server-side record is gone, so the stale cookie no longer works.
        let (status, _, _) = send(&app, Method::GET, "/api/me", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Logout stays idempotent without a session.
        let (status, _, _) = send(&app, Method::POST, "/api/auth/logout", None, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        drop(db_dir);
    }

    #[tokio::test]
    async fn login_failures_are_generic_and_success_returns_a_working_session() {
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);
        register(&app, "Kokiri", "longpassword1").await;

        let (status, _, body) = send(
            &app,
            Method::POST,
            "/api/auth/login",
            Some(json!({ "username": "Kokiri", "password": "wrongpassword" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "invalid credentials");

        let (status, _, body) = send(
            &app,
            Method::POST,
            "/api/auth/login",
            Some(json!({ "username": "nobody", "password": "longpassword1" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body["error"], "invalid credentials",
            "unknown users get the same message as wrong passwords"
        );

        // Case-insensitive username, correct password.
        let (status, cookie, body) = send(
            &app,
            Method::POST,
            "/api/auth/login",
            Some(json!({ "username": "kokiri", "password": "longpassword1" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["user"]["username"], "Kokiri");
        let cookie = cookie.expect("login should start a session");

        let (status, _, _) = send(&app, Method::GET, "/api/me", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);

        drop(db_dir);
    }

    #[tokio::test]
    async fn registration_validation_and_duplicates() {
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);

        let (status, _, _) = register(&app, "Beedle", "longpassword1").await;
        assert_eq!(status, StatusCode::CREATED);

        // Duplicate, case-insensitive → 409.
        let (status, _, body) = register(&app, "beedle", "longpassword1").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].is_string());

        // Second distinct user is not admin.
        let (status, _, body) = register(&app, "Paya", "longpassword1").await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["user"]["role"], "user");

        // Validation failures → 400 with a message.
        for (username, password) in [
            ("ab", "longpassword1"),
            ("bad name", "longpassword1"),
            ("okname", "short"),
        ] {
            let (status, _, body) = register(&app, username, password).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{username:?}/{password:?}");
            assert!(body["error"].is_string(), "{body}");
        }

        drop(db_dir);
    }

    #[tokio::test]
    async fn save_routes_follow_mode_policy() {
        // Local: anonymous browsing still works.
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);
        let (status, _, _) = send(&app, Method::GET, "/api/slots", None, None).await;
        assert_eq!(status, StatusCode::OK);

        // LAN: anonymous → 401; signed in → 200.
        let (db_dir2, state2) = test_state(Mode::Lan, None).await;
        let app2 = router(state2);
        let (status, _, body) = send(&app2, Method::GET, "/api/slots", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "authentication required");

        let (_, cookie, _) = register(&app2, "LanUser", "longpassword1").await;
        let cookie = cookie.expect("session cookie");
        let (status, _, _) = send(&app2, Method::GET, "/api/slots", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);

        // Hub: same policy.
        let (db_dir3, state3) = test_state(Mode::Hub, None).await;
        let app3 = router(state3);
        let (status, _, _) = send(&app3, Method::GET, "/api/slots", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (_, cookie, _) = register(&app3, "HubUser", "longpassword1").await;
        let cookie = cookie.expect("session cookie");
        let (status, _, _) = send(&app3, Method::GET, "/api/slots", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);

        // Completion endpoint is behind the same guard.
        let (status, _, _) = send(&app2, Method::GET, "/api/completion", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Per-object map completion is behind the same guard too.
        let (status, _, body) = send(&app2, Method::GET, "/api/slots/0/map", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "authentication required");

        drop(db_dir);
        drop(db_dir2);
        drop(db_dir3);
    }

    #[tokio::test]
    async fn profiles_require_auth_in_every_mode() {
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);

        // Even local mode gates per-user resources.
        let (status, _, body) = send(&app, Method::GET, "/api/profiles", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "authentication required");

        let (_, cookie, _) = register(&app, "ProfileUser", "longpassword1").await;
        let cookie = cookie.expect("session cookie");

        let (status, _, body) = send(&app, Method::GET, "/api/profiles", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        let profiles = body.as_array().expect("array of profiles");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0]["name"], "Default");

        let (status, _, body) = send(
            &app,
            Method::POST,
            "/api/profiles",
            Some(json!({ "name": "Master Mode", "game_mode": "master" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["name"], "Master Mode");
        assert_eq!(body["game_mode"], "master");

        let (status, _, _) = send(
            &app,
            Method::POST,
            "/api/profiles",
            Some(json!({ "name": "Bad", "game_mode": "hero" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _, _) = send(
            &app,
            Method::POST,
            "/api/profiles",
            Some(json!({ "name": "   " })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Unauthenticated profile creation is rejected before any validation.
        let (status, _, _) = send(
            &app,
            Method::POST,
            "/api/profiles",
            Some(json!({ "name": "Sneaky" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _, body) = send(&app, Method::GET, "/api/profiles", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 2);

        drop(db_dir);
    }
}
