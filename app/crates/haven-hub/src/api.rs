//! The Hub's web API. Apps pair once with the code on the status page,
//! then send their key with every request (`Authorization: Bearer <key>`).
//!
//! To an app the Hub looks like a media server: libraries of titles,
//! details, search and the ways to watch something, all as plain rows.

use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use haven_core::hub::{Include, Setup};
use serde::Deserialize;
use serde_json::json;

use crate::{hub::Hub, page};

type Shared = State<Arc<Hub>>;

pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/", get(status_page))
        .route("/addons/add", post(page_add_addon))
        .route("/addons/remove", post(page_remove_addon))
        .route("/devices/forget", post(page_forget_device))
        .route("/api/hello", get(hello))
        .route("/api/pair", post(pair))
        .route("/api/libraries", get(libraries))
        .route("/api/catalog", get(catalog))
        .route("/api/search", get(search))
        .route("/api/meta/{kind}/{id}", get(meta))
        .route("/api/streams/{kind}/{id}", get(streams))
        .route("/api/calendar", get(calendar))
        .route("/api/setup", get(get_setup).put(put_setup).post(put_setup))
        .with_state(hub)
}

fn refused() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"error": "this device is not paired with the Hub"}))).into_response()
}

fn allowed(hub: &Hub, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|key| hub.knows(key.trim()))
}

fn failed(message: impl Into<String>) -> Response {
    (StatusCode::BAD_GATEWAY, Json(json!({"error": message.into()}))).into_response()
}

// ---- Status page ---------------------------------------------------------

async fn status_page(State(hub): Shared) -> Html<String> {
    Html(page::status(&hub))
}

#[derive(Deserialize)]
struct AddonForm {
    address: String,
}

async fn page_add_addon(State(hub): Shared, Form(form): Form<AddonForm>) -> Redirect {
    match hub.install(&form.address).await {
        Ok(name) => eprintln!("haven-hub: installed {name}"),
        Err(err) => eprintln!("haven-hub: could not install that add-on: {err}"),
    }
    Redirect::to("/")
}

async fn page_remove_addon(State(hub): Shared, Form(form): Form<AddonForm>) -> Redirect {
    hub.remove(&form.address);
    Redirect::to("/")
}

#[derive(Deserialize)]
struct DeviceForm {
    index: usize,
}

async fn page_forget_device(State(hub): Shared, Form(form): Form<DeviceForm>) -> Redirect {
    hub.forget_device(form.index);
    Redirect::to("/")
}

// ---- Pairing ---------------------------------------------------------------

/// Answers anyone: lets an app tell a Hub from another server.
async fn hello(State(hub): Shared) -> Json<serde_json::Value> {
    Json(json!({"hub": "Haven Hub", "name": hub.name, "version": env!("CARGO_PKG_VERSION")}))
}

#[derive(Deserialize)]
struct PairAsk {
    code: String,
    #[serde(default)]
    device: String,
}

async fn pair(State(hub): Shared, Json(ask): Json<PairAsk>) -> Response {
    match hub.pair(&ask.code, &ask.device) {
        Some(key) => Json(json!({"key": key, "name": hub.name})).into_response(),
        None => (StatusCode::UNAUTHORIZED, Json(json!({"error": "that pairing code is not the one on the Hub's page"})))
            .into_response(),
    }
}

// ---- Titles ----------------------------------------------------------------

async fn libraries(State(hub): Shared, headers: HeaderMap) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    let rows: Vec<_> = hub
        .offered()
        .into_iter()
        .map(|o| json!({"id": o.id, "name": o.catalog.title(), "genres": o.catalog.genres()}))
        .collect();
    Json(rows).into_response()
}

#[derive(Deserialize)]
struct CatalogAsk {
    library: String,
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    skip: usize,
}

async fn catalog(State(hub): Shared, headers: HeaderMap, Query(ask): Query<CatalogAsk>) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    match hub.catalog(&ask.library, ask.genre.as_deref().filter(|g| !g.is_empty()), ask.skip).await {
        Ok(titles) => Json(json!({"metas": titles})).into_response(),
        Err(err) => failed(err),
    }
}

#[derive(Deserialize)]
struct SearchAsk {
    q: String,
}

async fn search(State(hub): Shared, headers: HeaderMap, Query(ask): Query<SearchAsk>) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    Json(json!({"metas": hub.search(ask.q.trim()).await})).into_response()
}

async fn meta(State(hub): Shared, headers: HeaderMap, Path((kind, id)): Path<(String, String)>) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    match hub.meta(&kind, &id).await {
        Ok(meta) => Json(json!({"meta": meta})).into_response(),
        Err(err) => failed(err),
    }
}

async fn streams(State(hub): Shared, headers: HeaderMap, Path((kind, id)): Path<(String, String)>) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    Json(json!({"streams": hub.streams(&kind, &id).await})).into_response()
}

/// The followed series' details, so an app's calendar has fresh dates.
async fn calendar(State(hub): Shared, headers: HeaderMap) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    Json(json!({"series": hub.calendar()})).into_response()
}

// ---- Setup -----------------------------------------------------------------

/// The Hub's whole setup, for a device that wants it (add-ons included:
/// an app without add-ons ignores them).
async fn get_setup(State(hub): Shared, headers: HeaderMap) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    let store = hub.store.lock().unwrap();
    match Setup::read(&store, Include { sources: true, hubs: false }) {
        Ok(setup) => Json(setup).into_response(),
        Err(err) => failed(err.to_string()),
    }
}

/// Adds a device's setup to the Hub's ("Send everything to the Hub").
async fn put_setup(State(hub): Shared, headers: HeaderMap, Json(setup): Json<Setup>) -> Response {
    if !allowed(&hub, &headers) {
        return refused();
    }
    let store = hub.store.lock().unwrap();
    match setup.merge_into(&store, Include { sources: true, hubs: false }) {
        Ok(merged) => {
            let added = merged.added;
            drop(store);
            hub.after_merge(merged);
            Json(json!({"added": added})).into_response()
        }
        Err(err) => failed(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use haven_core::storage::Store;
    use tower::ServiceExt;

    use super::*;

    async fn call(app: &Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let answer = app.clone().oneshot(request).await.unwrap();
        let status = answer.status();
        let bytes = axum::body::to_bytes(answer.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn only_paired_devices_get_in_and_setup_round_trips() {
        let hub = Arc::new(Hub::new(Store::in_memory().unwrap(), haven_core::http::client().unwrap(), "Den".into()));
        let app = router(hub.clone());

        let (status, _) = call(&app, Request::get("/api/libraries").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let wrong = Request::post("/api/pair")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"code":"x","device":"Phone"}"#))
            .unwrap();
        assert_eq!(call(&app, wrong).await.0, StatusCode::UNAUTHORIZED);

        let body = json!({"code": hub.pairing_code(), "device": "Phone"}).to_string();
        let ask = Request::post("/api/pair").header("content-type", "application/json").body(Body::from(body)).unwrap();
        let (status, answer) = call(&app, ask).await;
        assert_eq!(status, StatusCode::OK);
        let key = answer["key"].as_str().unwrap().to_owned();
        let bearer = format!("Bearer {key}");

        let (status, rows) =
            call(&app, Request::get("/api/libraries").header("authorization", &bearer).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(rows, json!([]));

        let setup = json!({"servers": [{"kind": "jellyfin", "name": "Den", "address": "http://192.168.1.9:8096", "user_id": "u", "token": "t"}]});
        let put = Request::put("/api/setup")
            .header("authorization", &bearer)
            .header("content-type", "application/json")
            .body(Body::from(setup.to_string()))
            .unwrap();
        let (status, answer) = call(&app, put).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["added"], 1);
        let (_, back) =
            call(&app, Request::get("/api/setup").header("authorization", &bearer).body(Body::empty()).unwrap()).await;
        assert_eq!(back["servers"][0]["address"], "http://192.168.1.9:8096");
    }
}
