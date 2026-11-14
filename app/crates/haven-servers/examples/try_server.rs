//! Tries a Jellyfin or Emby server from the command line, without the app:
//!
//! ```text
//! cargo run -p haven-servers --example try_server -- jellyfin <address> <user> [password]
//! ```
//!
//! Prints the libraries, the newest titles of each and the details of the
//! first one. Never prints the token.

use haven_servers::{Client, Kind, split_id};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kind, address, user, rest @ ..] = args.as_slice() else {
        eprintln!("usage: try_server <jellyfin|emby> <address> <user> [password]");
        std::process::exit(2);
    };
    let kind = Kind::parse(kind).filter(|kind| *kind != Kind::Plex).ok_or("jellyfin or emby")?;
    let password = rest.first().map(String::as_str).unwrap_or_default();

    let client = Client::new(haven_core::http::client()?, "haven-try-server".into());
    let server = client.sign_in(kind, address, user, password).await?;
    println!("signed in to \"{}\" at {}", server.name, server.address);

    let mut first = None;
    for library in client.libraries(&server).await? {
        let latest = client.latest(&server, &library.id).await?;
        let all = client.all(&server, &library.id).await?;
        println!("library \"{}\": {} newest, {} by name", library.name, latest.len(), all.len());
        for title in latest.iter().take(3) {
            println!("  {} [{}] {}", title.name, title.kind, title.release_info.as_deref().unwrap_or("-"));
        }
        first = first.or_else(|| all.into_iter().next());
    }
    if let Some(title) = first {
        let (_, item) = split_id(&title.id).ok_or("bad id")?;
        let meta = client.meta(&server, item).await?;
        println!(
            "details of \"{}\": {} {:?} {:?}, {} genres, {} cast, {} episodes, background {}",
            meta.name,
            meta.kind,
            meta.release_info,
            meta.runtime,
            meta.genres.len(),
            meta.cast.len(),
            meta.videos.len(),
            meta.background.is_some(),
        );
        let video = meta.videos.first().map_or(title.id.as_str(), |video| video.id.as_str());
        let (_, video) = split_id(video).ok_or("bad id")?;
        let streams = client.streams(&server, video).await?;
        let address = streams[0].url.as_deref().unwrap_or_default();
        println!("stream: {}…", address.split("api_key=").next().unwrap_or_default());
        let found = client.search(&server, &meta.name).await?;
        println!("search for its name finds {} title(s)", found.len());
    }
    Ok(())
}
