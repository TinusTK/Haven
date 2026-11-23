//! Haven Hub: Haven without a screen, running all the time on a Raspberry
//! Pi or a small server. It keeps the add-ons, media server sign-ins,
//! TV sources, keys, watchlist and watch progress, and Haven's apps on
//! phones and computers use it like a media server.
//!
//! Settings come from the environment:
//! - `HAVEN_HUB_DATA`: folder for its database (default `/data`, or
//!   `./haven-hub-data` when `/data` cannot be written).
//! - `HAVEN_HUB_PORT`: port to listen on (default 8590).
//! - `HAVEN_HUB_NAME`: the name apps show (default "Haven Hub").

mod api;
mod background;
mod hub;
mod page;
mod settings;

use std::{path::PathBuf, sync::Arc};

use haven_core::storage::Store;

fn data_folder() -> PathBuf {
    if let Some(folder) = std::env::var_os("HAVEN_HUB_DATA") {
        return PathBuf::from(folder);
    }
    let docker = PathBuf::from("/data");
    if std::fs::create_dir_all(&docker).is_ok() && tempfile_ok(&docker) {
        docker
    } else {
        PathBuf::from("haven-hub-data")
    }
}

fn tempfile_ok(folder: &std::path::Path) -> bool {
    let probe = folder.join(".haven-hub-probe");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = data_folder();
    std::fs::create_dir_all(&folder)?;
    let store = Store::open(&folder.join("haven-hub.db"))?;
    let port: u16 = std::env::var("HAVEN_HUB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(haven_core::hub::DEFAULT_PORT);
    let name = std::env::var("HAVEN_HUB_NAME").ok().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Haven Hub".into());
    let http = haven_core::http::client()?;
    let hub = Arc::new(hub::Hub::new(store, http, name));

    // Background work while no one is looking.
    background::start(hub.clone());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    eprintln!("haven-hub: data in {}", folder.display());
    eprintln!("haven-hub: listening on port {port}; pairing code {}", hub.pairing_code());
    axum::serve(listener, api::router(hub))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
