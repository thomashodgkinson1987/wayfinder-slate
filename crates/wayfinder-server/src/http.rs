use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
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
use crate::ingest::{
    self, IngestError, IngestOptions, IngestSource, IngestTarget, MAX_CAPTION_IMAGE_BYTES,
    MAX_CAPTION_SAV_BYTES, MAX_GAME_DATA_BYTES, SavePayload,
};
use crate::saves::{self, SlotScope};

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
        .route("/api/profiles/{id}/slots", get(list_profile_slots))
        .route(
            "/api/profiles/{id}/slots/{slot}",
            delete(delete_profile_slot),
        )
        .route(
            "/api/profiles/{id}/slots/{slot}/caption",
            get(profile_slot_caption),
        )
        .route(
            "/api/profiles/{id}/saves/upload",
            post(upload_save).layer(DefaultBodyLimit::max(state.config.max_upload_bytes)),
        )
        .route_layer(middleware::from_fn(require_auth_always));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(public_config))
        .route("/api/map-objects", get(map_objects))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/saves/import-path", post(import_path))
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

enum ResolvedScope {
    Machine,
    Profile(String),
}

impl ResolvedScope {
    fn as_scope(&self) -> SlotScope<'_> {
        match self {
            Self::Machine => SlotScope::Machine,
            Self::Profile(id) => SlotScope::Profile(id),
        }
    }
}

/// Local/LAN read the machine's save folder; hub reads the signed-in user's
/// primary profile.
async fn resolve_scope(
    state: &AppState,
    auth_session: &AuthSession,
) -> Result<ResolvedScope, Box<Response>> {
    if state.config.mode != Mode::Hub {
        return Ok(ResolvedScope::Machine);
    }

    let Some(user) = auth_session.user.as_ref() else {
        return Err(Box::new(json_error(
            StatusCode::UNAUTHORIZED,
            "authentication required",
        )));
    };

    match state.backend.primary_profile_for_user(&user.id).await {
        Ok(Some(profile)) => Ok(ResolvedScope::Profile(profile.id)),
        Ok(None) => Err(Box::new(json_error(
            StatusCode::NOT_FOUND,
            "profile not found",
        ))),
        Err(error) => {
            tracing::error!(%error, "could not resolve the primary profile");
            Err(Box::new(internal_error()))
        }
    }
}

async fn owns_profile(state: &AppState, auth_session: &AuthSession, profile_id: &str) -> bool {
    let Some(user) = auth_session.user.as_ref() else {
        return false;
    };
    matches!(
        state.backend.profile_for_user(&user.id, profile_id).await,
        Ok(Some(_))
    )
}

async fn list_slots(State(state): State<AppState>, auth_session: AuthSession) -> Response {
    let scope = match resolve_scope(&state, &auth_session).await {
        Ok(scope) => scope,
        Err(response) => return *response,
    };

    let result = match scope.as_scope() {
        SlotScope::Machine => saves::list_machine_slots(&state.db).await,
        SlotScope::Profile(profile_id) => saves::list_profile_slots(&state.db, profile_id).await,
    };

    match result {
        Ok(slots) => no_store(Json(slots).into_response()),
        Err(error) => slot_error_response(0, error),
    }
}

async fn slot_detail(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path(slot): Path<usize>,
) -> Response {
    let scope = match resolve_scope(&state, &auth_session).await {
        Ok(scope) => scope,
        Err(response) => return *response,
    };

    let result = match scope.as_scope() {
        SlotScope::Machine => saves::load_machine_slot(&state.db, slot).await,
        SlotScope::Profile(profile_id) => {
            saves::load_profile_slot(&state.db, profile_id, slot).await
        }
    };

    match result {
        Ok(detail) => no_store(Json(detail).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn slot_map(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path(slot): Path<usize>,
) -> Response {
    let scope = match resolve_scope(&state, &auth_session).await {
        Ok(scope) => scope,
        Err(response) => return *response,
    };

    let result = match scope.as_scope() {
        SlotScope::Machine => saves::load_machine_map(&state.db, slot).await,
        SlotScope::Profile(profile_id) => {
            saves::load_profile_map(&state.db, profile_id, slot).await
        }
    };

    match result {
        Ok(completion) => no_store(Json(completion).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn slot_caption(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path(slot): Path<usize>,
) -> Response {
    let scope = match resolve_scope(&state, &auth_session).await {
        Ok(scope) => scope,
        Err(response) => return *response,
    };

    let result = match scope.as_scope() {
        SlotScope::Machine => saves::load_machine_caption(&state.db, slot).await,
        SlotScope::Profile(profile_id) => {
            saves::load_profile_caption(&state.db, profile_id, slot).await
        }
    };

    match result {
        Ok(bytes) => image_response(bytes),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn completion(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Query(query): Query<CompletionQuery>,
) -> Response {
    let slot = query.slot.unwrap_or(0);
    let scope = match resolve_scope(&state, &auth_session).await {
        Ok(scope) => scope,
        Err(response) => return *response,
    };

    let result = match scope.as_scope() {
        SlotScope::Machine => saves::load_machine_completion(&state.db, slot).await,
        SlotScope::Profile(profile_id) => {
            saves::load_profile_completion(&state.db, profile_id, slot).await
        }
    };

    match result {
        Ok(summary) => no_store(Json(summary).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn list_profile_slots(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path(profile_id): Path<String>,
) -> Response {
    if !owns_profile(&state, &auth_session, &profile_id).await {
        return json_error(StatusCode::NOT_FOUND, "profile not found");
    }

    match saves::list_profile_slots(&state.db, &profile_id).await {
        Ok(slots) => no_store(Json(slots).into_response()),
        Err(error) => slot_error_response(0, error),
    }
}

async fn profile_slot_caption(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path((profile_id, slot)): Path<(String, usize)>,
) -> Response {
    if !owns_profile(&state, &auth_session, &profile_id).await {
        return json_error(StatusCode::NOT_FOUND, "profile not found");
    }

    match saves::load_profile_caption(&state.db, &profile_id, slot).await {
        Ok(bytes) => image_response(bytes),
        Err(error) => slot_error_response(slot, error),
    }
}

/// Remove a stored profile slot (an upload or, later, an agent sync) the
/// caller owns. The slot's snapshots are removed with it.
async fn delete_profile_slot(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path((profile_id, slot)): Path<(String, usize)>,
) -> Response {
    if !owns_profile(&state, &auth_session, &profile_id).await {
        return json_error(StatusCode::NOT_FOUND, "profile not found");
    }

    match saves::delete_profile_slot(&state.db, &profile_id, slot).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "save slot not found"),
        Err(error) => slot_error_response(slot, error),
    }
}

fn image_response(bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    no_store(response)
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
            tracing::warn!(slot, %error, "could not read stored save data");
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "stored save data could not be read",
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

pub(crate) fn map_dataset() -> &'static saves::MapObjects {
    static OBJECTS: OnceLock<saves::MapObjects> = OnceLock::new();

    // Parse the canonical committed dataset at compile time. This keeps slot
    // labels and per-object completion working even when the frontend has not
    // been built (for example in the Rust CI job), where `frontend/dist` only
    // holds a placeholder.
    OBJECTS.get_or_init(|| {
        match saves::MapObjects::from_json(include_bytes!(
            "../../../frontend/static/data/map_objects.json"
        )) {
            Ok(objects) => objects,
            Err(error) => {
                tracing::warn!(%error, "could not parse the bundled map objects dataset");
                saves::MapObjects::default()
            }
        }
    })
}

#[derive(Deserialize)]
struct ImportPathRequest {
    path: String,
}

#[derive(Serialize)]
struct ImportErrorItem {
    slot: usize,
    error: String,
}

#[derive(Serialize)]
struct ImportResponse {
    imported: Vec<saves::SlotSummary>,
    errors: Vec<ImportErrorItem>,
}

/// Import a save folder into the machine-local source. Local mode only.
async fn import_path(
    State(state): State<AppState>,
    Json(payload): Json<ImportPathRequest>,
) -> Response {
    if state.config.mode != Mode::Local {
        return json_error(
            StatusCode::FORBIDDEN,
            "path import is only available in local mode",
        );
    }

    let path = std::path::PathBuf::from(payload.path);
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "path does not exist"),
    };
    if !canonical.is_dir() {
        return json_error(StatusCode::BAD_REQUEST, "path is not a directory");
    }

    let slots = match crate::config::try_discover_slots(&canonical) {
        Ok(slots) => slots,
        Err(error) => {
            tracing::warn!(%error, "could not list the import path");
            return json_error(StatusCode::BAD_REQUEST, "could not list the save folder");
        }
    };
    if slots.len() > 64 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "path contains more than 64 save slots",
        );
    }

    let options = IngestOptions::from(&*state.config);
    let mut imported = Vec::new();
    let mut errors = Vec::new();

    for slot in slots {
        if slot.index > ingest::MAX_SLOT_INDEX as usize {
            errors.push(ImportErrorItem {
                slot: slot.index,
                error: "slot index is out of range".to_owned(),
            });
            continue;
        }

        let target = IngestTarget {
            profile_id: None,
            slot_index: Some(slot.index as u32),
            source: IngestSource::Local,
        };

        match ingest::ingest_slot_dir(&state.db, map_dataset(), target, &slot.dir, options).await {
            Ok(outcome) => {
                match saves::load_machine_slot(&state.db, outcome.slot_index as usize).await {
                    Ok(detail) => imported.push(detail.summary),
                    Err(error) => errors.push(ImportErrorItem {
                        slot: slot.index,
                        error: error.to_string(),
                    }),
                }
            }
            Err(error) => {
                tracing::warn!(slot = slot.index, %error, "could not import save slot");
                errors.push(ImportErrorItem {
                    slot: slot.index,
                    error: error.to_string(),
                });
            }
        }
    }

    Json(ImportResponse { imported, errors }).into_response()
}

/// Store a multipart save upload for a profile the caller owns.
async fn upload_save(
    State(state): State<AppState>,
    auth_session: AuthSession,
    Path(profile_id): Path<String>,
    mut multipart: Multipart,
) -> Response {
    if !owns_profile(&state, &auth_session, &profile_id).await {
        return json_error(StatusCode::NOT_FOUND, "profile not found");
    }

    let mut game_data: Option<Vec<u8>> = None;
    let mut caption_sav: Option<Vec<u8>> = None;
    let mut caption_image: Option<Vec<u8>> = None;
    let mut slot_index: Option<u32> = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    format!("invalid multipart form: {error}"),
                );
            }
        };

        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "slot_index" => {
                if slot_index.is_some() {
                    return json_error(StatusCode::BAD_REQUEST, "duplicate slot_index part");
                }
                let text = match field.text().await {
                    Ok(text) => text,
                    Err(error) => {
                        return json_error(
                            StatusCode::BAD_REQUEST,
                            format!("invalid slot_index: {error}"),
                        );
                    }
                };
                match text.trim().parse::<u32>() {
                    Ok(value) => slot_index = Some(value),
                    Err(_) => {
                        return json_error(
                            StatusCode::BAD_REQUEST,
                            "slot_index must be a non-negative integer",
                        );
                    }
                }
            }
            "game_data" => {
                if game_data.is_some() {
                    return json_error(StatusCode::BAD_REQUEST, "duplicate game_data part");
                }
                match read_part(field, MAX_GAME_DATA_BYTES).await {
                    Ok(bytes) => game_data = Some(bytes),
                    Err(response) => return *response,
                }
            }
            "caption_sav" => {
                if caption_sav.is_some() {
                    return json_error(StatusCode::BAD_REQUEST, "duplicate caption_sav part");
                }
                match read_part(field, MAX_CAPTION_SAV_BYTES).await {
                    Ok(bytes) => caption_sav = Some(bytes),
                    Err(response) => return *response,
                }
            }
            "caption_image" => {
                if caption_image.is_some() {
                    return json_error(StatusCode::BAD_REQUEST, "duplicate caption_image part");
                }
                match read_part(field, MAX_CAPTION_IMAGE_BYTES).await {
                    Ok(bytes) => {
                        if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                            return json_error(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                "caption_image must be a JPEG image",
                            );
                        }
                        caption_image = Some(bytes);
                    }
                    Err(response) => return *response,
                }
            }
            other => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    format!("unknown form part {other:?}"),
                );
            }
        }
    }

    let Some(game_data) = game_data else {
        return json_error(StatusCode::BAD_REQUEST, "game_data part is required");
    };

    let payload = SavePayload {
        game_data,
        caption_sav,
        caption_image,
        last_modified: time::OffsetDateTime::now_utc().unix_timestamp(),
    };
    let target = IngestTarget {
        profile_id: Some(profile_id.clone()),
        slot_index,
        source: IngestSource::Upload,
    };

    match ingest::ingest(
        &state.db,
        map_dataset(),
        target,
        payload,
        IngestOptions::from(&*state.config),
    )
    .await
    {
        Ok(outcome) => {
            match saves::load_profile_slot(&state.db, &profile_id, outcome.slot_index as usize)
                .await
            {
                Ok(detail) => {
                    let status = if outcome.created {
                        StatusCode::CREATED
                    } else {
                        StatusCode::OK
                    };
                    (status, Json(detail.summary)).into_response()
                }
                Err(error) => slot_error_response(outcome.slot_index as usize, error),
            }
        }
        Err(error) => ingest_error_response(error),
    }
}

async fn read_part(
    field: axum::extract::multipart::Field<'_>,
    limit: usize,
) -> Result<Vec<u8>, Box<Response>> {
    match field.bytes().await {
        Ok(bytes) if bytes.len() <= limit => Ok(bytes.to_vec()),
        Ok(_) => Err(Box::new(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "uploaded part exceeds the size limit",
        ))),
        Err(error) => Err(Box::new(json_error(
            StatusCode::BAD_REQUEST,
            format!("could not read form part: {error}"),
        ))),
    }
}

fn ingest_error_response(error: IngestError) -> Response {
    match error {
        IngestError::InvalidSave(error) => {
            tracing::debug!(%error, "rejected uploaded save");
            json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "save file is not a supported Breath of the Wild save",
            )
        }
        IngestError::InvalidTarget(message) => json_error(StatusCode::BAD_REQUEST, message),
        IngestError::Io(error) => {
            tracing::warn!(%error, "could not read an uploaded save");
            json_error(StatusCode::BAD_REQUEST, "could not read the uploaded save")
        }
        IngestError::Db(error) => {
            tracing::error!(%error, "could not store an uploaded save");
            internal_error()
        }
        IngestError::Join(error) => {
            tracing::error!(%error, "save ingestion task failed");
            internal_error()
        }
    }
}
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
            snapshot_limit: 500,
            keep_raw_saves: false,
            watch_saves: false,
            max_upload_bytes: 8 * 1024 * 1024,
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

    async fn raw_get(app: &Router, uri: &str, cookie: Option<&str>) -> (StatusCode, Vec<u8>) {
        let mut builder = Request::builder().method(Method::GET).uri(uri);
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        (status, body_bytes(response).await)
    }

    /// Encode `(name, bytes)` pairs as a `multipart/form-data` body.
    fn multipart(parts: &[(&str, &[u8])]) -> (String, Vec<u8>) {
        let boundary = "----wayfinder-test-boundary";
        let mut body = Vec::new();
        for (name, bytes) in parts {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            );
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    /// Send a raw (non-JSON-encoded) request through the full router.
    async fn send_raw(
        app: &Router,
        method: Method,
        uri: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        cookie: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = body_bytes(response).await;
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, json)
    }

    async fn snapshot_count(db: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM snapshots")
            .fetch_one(db)
            .await
            .unwrap()
    }

    /// Register a user and return its session cookie and primary profile id.
    async fn account(app: &Router, username: &str) -> (String, String) {
        let (_, cookie, _) = register(app, username, "longpassword1").await;
        let cookie = cookie.expect("session cookie");
        let (status, _, profiles) =
            send(app, Method::GET, "/api/profiles", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        let profile_id = profiles[0]["id"].as_str().unwrap().to_string();
        (cookie, profile_id)
    }

    #[tokio::test]
    async fn slot_endpoints_serve_stored_snapshots() {
        let root = temp_root("http");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            Some(b"\xff\xd8\xffjpeg-bytes"),
        );
        // Corrupt files are rejected at ingestion and never appear as slots.
        write_slot(&root.join("save_0"), 1, b"not a save", None, None);
        write_slot(
            &root.join("save_0"),
            2,
            &valid_save(),
            Some(&caption_save()),
            None,
        );

        let (db_dir, state) = test_state(Mode::Local, Some(root.clone())).await;
        let report =
            crate::ingest::scan_machine_slots(&state.config, &state.db, map_dataset()).await;
        assert_eq!(report.slots, 3);
        assert_eq!(report.ingested, 2);
        assert_eq!(report.errors, 1);

        let app = router(state);

        let (status, _, slots) = send(&app, Method::GET, "/api/slots", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(slots.as_array().unwrap().len(), 2);
        assert_eq!(slots[0]["slot"].as_u64(), Some(0));
        assert_eq!(slots[0]["label"], "Dueling Peaks Stable");

        let (status, _, detail) = send(&app, Method::GET, "/api/slots/0", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(detail["slot"].as_u64(), Some(0));
        assert_eq!(detail["stats"]["play_time_seconds"].as_u64(), Some(50_166));
        assert_eq!(detail["stats"]["hearts"]["current"].as_f64(), Some(4.0));
        assert_eq!(detail["stats"]["location"]["map"], "G-6");
        assert_eq!(
            detail["completion"]["shrines"]["completed"].as_u64(),
            Some(1)
        );

        let (status, _, _) = send(&app, Method::GET, "/api/slots/9", None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _, _) = send(&app, Method::GET, "/api/slots/1", None, None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "corrupt saves are not stored"
        );

        let (status, _, map) = send(&app, Method::GET, "/api/slots/0/map", None, None).await;
        assert_eq!(status, StatusCode::OK);
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

        let (status, bytes) = raw_get(&app, "/api/slots/0/caption", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, b"\xff\xd8\xffjpeg-bytes".to_vec());

        let (status, _) = raw_get(&app, "/api/slots/2/caption", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = raw_get(&app, "/api/slots/9/caption", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(root).unwrap();
        drop(db_dir);
    }

    #[tokio::test]
    async fn hub_mode_requires_auth_and_starts_empty() {
        let (db_dir, state) = test_state(Mode::Hub, None).await;
        let app = router(state);

        let (status, _, body) = send(&app, Method::GET, "/api/slots", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "authentication required");

        let (_, cookie, _) = register(&app, "HubEmpty", "longpassword1").await;
        let cookie = cookie.expect("session cookie");
        let (status, _, body) = send(&app, Method::GET, "/api/slots", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!([]));

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

    // --- M4 upload, import, and hub scoping ------------------------------

    #[tokio::test]
    async fn upload_creates_replaces_and_allocates_slots() {
        let (db_dir, state) = test_state(Mode::Hub, None).await;
        let db = state.db.clone();
        let app = router(state);
        let (cookie, profile_id) = account(&app, "Uploader").await;
        let upload_uri = format!("/api/profiles/{profile_id}/saves/upload");

        // Create: full set of parts.
        let (ct, body) = multipart(&[
            ("game_data", &valid_save()),
            ("caption_sav", &caption_save()),
            ("caption_image", b"\xff\xd8\xffjpeg"),
        ]);
        let (status, json) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["slot"].as_u64(), Some(0));
        assert_eq!(json["label"], "Dueling Peaks Stable");
        assert_eq!(
            json["caption_url"].as_str().unwrap(),
            format!("/api/profiles/{profile_id}/slots/0/caption")
        );
        assert_eq!(snapshot_count(&db).await, 1);

        // Replace the same slot with identical bytes: no new snapshot.
        let (ct, body) = multipart(&[
            ("game_data", &valid_save()),
            ("slot_index", b"0"),
            ("caption_image", b"\xff\xd8\xffjpeg"),
        ]);
        let (status, json) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["slot"].as_u64(), Some(0));
        assert_eq!(snapshot_count(&db).await, 1);

        // Two more uploads with no slot_index allocate the next free indexes.
        for expected in [1, 2] {
            let (ct, body) = multipart(&[("game_data", &valid_save())]);
            let (status, json) = send_raw(
                &app,
                Method::POST,
                &upload_uri,
                Some(&ct),
                body,
                Some(&cookie),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            assert_eq!(json["slot"].as_u64(), Some(expected));
        }

        // Every auto-allocated slot stores a fully parsed snapshot, not just
        // a row: stats and completion come from the uploaded bytes.
        for expected in [1, 2] {
            let (status, _, detail) = send(
                &app,
                Method::GET,
                &format!("/api/slots/{expected}"),
                None,
                Some(&cookie),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(detail["slot"].as_u64(), Some(expected));
            assert_eq!(detail["stats"]["play_time_seconds"].as_u64(), Some(50_166));
            assert_eq!(
                detail["completion"]["shrines"]["completed"].as_u64(),
                Some(1)
            );
        }

        // The profile-scoped list and caption reflect stored snapshots.
        let (status, _, slots) = send(
            &app,
            Method::GET,
            &format!("/api/profiles/{profile_id}/slots"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(slots.as_array().unwrap().len(), 3);

        let (status, bytes) = raw_get(
            &app,
            &format!("/api/profiles/{profile_id}/slots/0/caption"),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, b"\xff\xd8\xffjpeg".to_vec());

        drop(db_dir);
    }

    #[tokio::test]
    async fn delete_removes_profile_slot_and_its_snapshots() {
        let (db_dir, state) = test_state(Mode::Hub, None).await;
        let db = state.db.clone();
        let app = router(state);
        let (cookie, profile_id) = account(&app, "Deleter").await;
        let slot_uri = format!("/api/profiles/{profile_id}/slots/0");

        let (ct, body) = multipart(&[
            ("game_data", &valid_save()),
            ("caption_image", b"\xff\xd8\xffjpeg"),
        ]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &format!("/api/profiles/{profile_id}/saves/upload"),
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(snapshot_count(&db).await, 1);

        // Another user's session cannot delete someone else's slot, and
        // anonymous requests are rejected by the auth layer outright.
        let (other_cookie, _) = account(&app, "Bystander").await;
        let (status, _) = send_raw(
            &app,
            Method::DELETE,
            &slot_uri,
            None,
            Vec::new(),
            Some(&other_cookie),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send_raw(&app, Method::DELETE, &slot_uri, None, Vec::new(), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // The owner deletes the slot: 204, snapshots cascade away, and every
        // read path for the slot number comes up empty afterwards.
        let (status, _) = send_raw(
            &app,
            Method::DELETE,
            &slot_uri,
            None,
            Vec::new(),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(snapshot_count(&db).await, 0);

        let (status, _, slots) = send(
            &app,
            Method::GET,
            &format!("/api/profiles/{profile_id}/slots"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(slots.as_array().unwrap().is_empty());

        for uri in ["/api/slots/0", "/api/slots/0/map", "/api/slots/0/caption"] {
            let (status, _) = raw_get(&app, uri, Some(&cookie)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "GET {uri} after delete");
        }

        // Deleting the same slot again reports it as gone.
        let (status, _) = send_raw(
            &app,
            Method::DELETE,
            &slot_uri,
            None,
            Vec::new(),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        drop(db_dir);
    }

    #[tokio::test]
    async fn upload_rejects_bad_input() {
        let (db_dir, state) = test_state(Mode::Hub, None).await;
        let app = router(state);
        let (cookie, profile_id) = account(&app, "Validator").await;
        let upload_uri = format!("/api/profiles/{profile_id}/saves/upload");

        // Missing game_data.
        let (ct, body) = multipart(&[("caption_sav", &caption_save())]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Unparseable save.
        let (ct, body) = multipart(&[("game_data", b"not a save")]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        // Wrong magic for the caption image.
        let (ct, body) = multipart(&[
            ("game_data", &valid_save()),
            ("caption_image", b"not a jpeg"),
        ]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        // Unknown part name.
        let (ct, body) = multipart(&[("game_data", &valid_save()), ("surprise", b"x")]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Foreign/unknown profile: 404, never leaking existence.
        let (ct, body) = multipart(&[("game_data", &valid_save())]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            "/api/profiles/does-not-exist/saves/upload",
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Another user's profile is also 404 for this caller.
        let (other_cookie, other_profile) = account(&app, "Other").await;
        let (ct, body) = multipart(&[("game_data", &valid_save())]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &format!("/api/profiles/{other_profile}/saves/upload"),
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let _ = other_cookie;

        // Oversize game_data (over the 4 MiB per-part limit).
        let (ct, body) = multipart(&[("game_data", &vec![0_u8; 4 * 1024 * 1024 + 1])]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &upload_uri,
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

        drop(db_dir);
    }

    #[tokio::test]
    async fn hub_slots_follow_the_primary_profile() {
        let (db_dir, state) = test_state(Mode::Hub, None).await;
        let app = router(state);
        let (cookie, primary) = account(&app, "HubScoped").await;

        let (ct, body) = multipart(&[("game_data", &valid_save())]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &format!("/api/profiles/{primary}/saves/upload"),
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        // `/api/slots` resolves to the caller's primary profile.
        let (status, _, slots) = send(&app, Method::GET, "/api/slots", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(slots.as_array().unwrap().len(), 1);

        // A slot uploaded to a second profile is invisible via `/api/slots`.
        let (status, _, second) = send(
            &app,
            Method::POST,
            "/api/profiles",
            Some(json!({ "name": "Second" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let second_id = second["id"].as_str().unwrap().to_string();

        let (ct, body) = multipart(&[("game_data", &valid_save())]);
        let (status, _) = send_raw(
            &app,
            Method::POST,
            &format!("/api/profiles/{second_id}/saves/upload"),
            Some(&ct),
            body,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, _, slots) = send(&app, Method::GET, "/api/slots", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(slots.as_array().unwrap().len(), 1);

        let (status, _, owned) = send(
            &app,
            Method::GET,
            &format!("/api/profiles/{second_id}/slots"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(owned.as_array().unwrap().len(), 1);

        // A different user cannot enumerate the profile.
        let (other_cookie, _) = account(&app, "HubIntruder").await;
        let (status, _, _) = send(
            &app,
            Method::GET,
            &format!("/api/profiles/{second_id}/slots"),
            None,
            Some(&other_cookie),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        drop(db_dir);
    }

    #[tokio::test]
    async fn import_path_is_local_only_and_reports_per_slot_errors() {
        let root = temp_root("http-import");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            None,
        );
        write_slot(&root.join("save_0"), 1, b"not a save", None, None);

        // Local mode imports the folder, reporting the bad slot separately.
        let (db_dir, state) = test_state(Mode::Local, None).await;
        let app = router(state);
        let (status, json) = send_raw(
            &app,
            Method::POST,
            "/api/saves/import-path",
            Some("application/json"),
            serde_json::to_vec(&json!({ "path": root.to_str().unwrap() })).unwrap(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["imported"].as_array().unwrap().len(), 1);
        assert_eq!(json["errors"].as_array().unwrap().len(), 1);
        assert_eq!(json["errors"][0]["slot"].as_u64(), Some(1));

        let (status, _, slots) = send(&app, Method::GET, "/api/slots", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(slots.as_array().unwrap().len(), 1);

        // Missing path is a 400.
        let (status, _) = send_raw(
            &app,
            Method::POST,
            "/api/saves/import-path",
            Some("application/json"),
            serde_json::to_vec(&json!({ "path": "/no/such/folder" })).unwrap(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        std::fs::remove_dir_all(root).unwrap();
        drop(db_dir);

        // LAN mode forbids path import outright.
        let (db_dir2, state2) = test_state(Mode::Lan, None).await;
        let app2 = router(state2);
        let (status, _) = send_raw(
            &app2,
            Method::POST,
            "/api/saves/import-path",
            Some("application/json"),
            serde_json::to_vec(&json!({ "path": "/tmp" })).unwrap(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        drop(db_dir2);
    }
}
