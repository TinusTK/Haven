//! Asks a Haven Hub for its libraries, the first one's titles, a title's
//! details and its ways to watch: `try_hub <address> <key>`.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [address, key] = args.as_slice() else {
        eprintln!("usage: try_hub <address> <key>");
        std::process::exit(2);
    };
    let client = haven_servers::Client::new(haven_core::http::client()?, "try-hub".into());
    let server = haven_servers::Server {
        id: 1,
        kind: haven_servers::Kind::Hub,
        name: "Hub".into(),
        address: address.clone(),
        user_id: String::new(),
        token: key.clone(),
    };
    println!("online: {}", client.online(&server).await);
    let libraries = client.libraries(&server).await?;
    println!("libraries: {}", libraries.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", "));
    let first = libraries.first().ok_or("no libraries")?;
    let titles = client.page(&server, &first.id, None, 0, 40).await?;
    println!("{}: {} titles, first {:?}", first.name, titles.len(), titles.first().map(|t| (&t.id, &t.name)));
    if let Some((_, item)) = titles.first().and_then(|t| haven_servers::split_id(&t.id)) {
        let meta = client.meta(&server, item).await?;
        println!("details: {} ({:?}), imdb {:?}", meta.name, meta.release_info, meta.imdb_id);
        let streams = client.streams(&server, item).await?;
        println!("ways to watch: {}", streams.len());
    }
    Ok(())
}
