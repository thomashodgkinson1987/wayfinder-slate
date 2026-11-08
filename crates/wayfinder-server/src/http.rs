use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;

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
}

#[derive(Deserialize)]
struct CompletionQuery {
    slot: Option<usize>,
}

pub fn router(config: Arc<Config>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(public_config))
        .route("/api/map-objects", get(map_objects))
        .route("/api/completion", get(completion))
        .fallback(static_asset)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(config)
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn public_config(State(config): State<Arc<Config>>) -> Json<PublicConfig> {
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
    })
}

async fn map_objects() -> impl IntoResponse {
    // Serve the static map_objects.json dataset
    match Assets::get("map/botw/markers/map_objects.json") {
        Some(asset) => {
            let mut response = Response::new(Body::from(asset.data.into_owned()));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                "application/json".parse().unwrap(),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".parse().unwrap(),
            );
            response
        }
        None => (StatusCode::NOT_FOUND, "map objects dataset not found").into_response(),
    }
}

async fn completion(
    State(config): State<Arc<Config>>,
    Query(query): Query<CompletionQuery>,
) -> impl IntoResponse {
    let slot = query.slot.unwrap_or(0);
    let save_path = config.save_dir.as_ref().map(|dir| dir.join(format!("save_0/{}/game_data.sav", slot)));
    
    if let Some(path) = save_path {
        if path.exists() {
            match wayfinder_save::SaveFile::from_path(&path) {
                Ok(save) => {
                    let summary = save.completion_summary();
                    return Json(summary).into_response();
                }
                Err(_) => {}
            }
        }
    }
    
    // Return empty/default completion if no save found
    Json(wayfinder_save::CompletionSummary {
        shrines: wayfinder_save::CompletionCount::new(136, 0),
        towers: wayfinder_save::CompletionCount::new(15, 0),
        koroks: wayfinder_save::CompletionCount::new(900, 0),
        memories: wayfinder_save::CompletionCount::new(14, 0),
        locations: wayfinder_save::CompletionCount::new(187, 0),
        enemies: wayfinder_save::CompletionCount::new(84, 0),
    }).into_response()
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
