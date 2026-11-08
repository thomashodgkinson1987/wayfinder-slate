use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::saves;

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
        .route("/api/slots", get(list_slots))
        .route("/api/slots/{slot}", get(slot_detail))
        .route("/api/slots/{slot}/caption", get(slot_caption))
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

async fn map_objects() -> Response {
    match Assets::get("data/map_objects.json") {
        Some(asset) => asset_response("data/map_objects.json", asset.data.into_owned()),
        None => (StatusCode::NOT_FOUND, "map objects dataset not found").into_response(),
    }
}

async fn list_slots(State(config): State<Arc<Config>>) -> Response {
    let slots = saves::list_slots(&config, map_object_labels());
    no_store(Json(slots).into_response())
}

async fn slot_detail(State(config): State<Arc<Config>>, Path(slot): Path<usize>) -> Response {
    match saves::load_slot(&config, slot, map_object_labels()) {
        Ok(detail) => no_store(Json(detail).into_response()),
        Err(error) => slot_error_response(slot, error),
    }
}

async fn slot_caption(State(config): State<Arc<Config>>, Path(slot): Path<usize>) -> Response {
    match saves::load_caption(&config, slot) {
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
    State(config): State<Arc<Config>>,
    Query(query): Query<CompletionQuery>,
) -> Response {
    let slot = query.slot.unwrap_or(0);
    match saves::load_completion(&config, slot) {
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

fn map_object_labels() -> &'static saves::MapObjectLabels {
    static LABELS: OnceLock<saves::MapObjectLabels> = OnceLock::new();

    LABELS.get_or_init(|| match Assets::get("data/map_objects.json") {
        Some(asset) => match saves::MapObjectLabels::from_json(&asset.data) {
            Ok(labels) => labels,
            Err(error) => {
                tracing::warn!(%error, "could not parse map objects for slot labels");
                saves::MapObjectLabels::default()
            }
        },
        None => saves::MapObjectLabels::default(),
    })
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

    fn test_config(mode: Mode, save_dir: Option<PathBuf>) -> Arc<Config> {
        Arc::new(Config {
            mode,
            bind: "127.0.0.1:0".parse().unwrap(),
            save_dir,
            control_addr: "127.0.0.1:7777".parse().unwrap(),
            data_dir: PathBuf::from("data"),
        })
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

        let config = test_config(Mode::Local, Some(root.clone()));

        let response = list_slots(State(config.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let slots: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert_eq!(slots.as_array().unwrap().len(), 3);

        let response = slot_detail(State(config.clone()), Path(0)).await;
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

        let response = slot_detail(State(config.clone()), Path(9)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_detail(State(config.clone()), Path(1)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let response = slot_caption(State(config.clone()), Path(0)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(body_bytes(response).await, b"jpeg-bytes".to_vec());

        let response = slot_caption(State(config.clone()), Path(2)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_caption(State(config), Path(9)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(root).unwrap();
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
        let config = test_config(Mode::Hub, Some(root.clone()));

        let response = list_slots(State(config.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, b"[]".to_vec());

        let response = slot_detail(State(config.clone()), Path(0)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = slot_caption(State(config), Path(0)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(root).unwrap();
    }
}
