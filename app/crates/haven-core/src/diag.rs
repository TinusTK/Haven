//! A small log file beside the database, for finding out afterwards why
//! something did not load (a picture, a server's answer). Haven has no
//! console on Windows, so this is the only place such notes can be read.
//! Notes must never hold passwords, tokens or keys: log an address
//! through [`plain_address`].

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// The log starts afresh once it has grown past this.
const LOG_BYTES: u64 = 512 * 1024;

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// Opens the log file. Until this is called, notes only go to stderr.
pub fn start(path: &Path) {
    let big = std::fs::metadata(path).is_ok_and(|file| file.len() > LOG_BYTES);
    let opened = OpenOptions::new().create(true).append(!big).write(true).truncate(big).open(path);
    if let Ok(file) = opened {
        let _ = LOG.set(Mutex::new(file));
        note(&format!("Haven {} started", env!("CARGO_PKG_VERSION")));
    }
}

/// Writes one line to stderr and to the log file.
pub fn note(text: &str) {
    eprintln!("haven: {text}");
    if let Some(Ok(mut file)) = LOG.get().map(Mutex::lock) {
        let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let _ = writeln!(file, "{seconds} {text}");
    }
}

/// An address without what follows `?` (where tokens and keys travel)
/// and without `user:password@`.
pub fn plain_address(address: &str) -> String {
    let address = address.split(['?', '#']).next().unwrap_or(address);
    match address.split_once("://") {
        Some((scheme, rest)) => {
            let (host, path) = rest.split_once('/').map_or((rest, ""), |(host, path)| (host, path));
            let host = host.rsplit('@').next().unwrap_or(host);
            if path.is_empty() { format!("{scheme}://{host}") } else { format!("{scheme}://{host}/{path}") }
        }
        None => address.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logged_addresses_carry_no_secrets() {
        assert_eq!(
            plain_address("https://me:secret@tv.example.org:8096/Items/1/Images/Primary?api_key=abc&tag=1"),
            "https://tv.example.org:8096/Items/1/Images/Primary"
        );
        assert_eq!(plain_address("http://tv.example.org?x=1"), "http://tv.example.org");
        assert_eq!(plain_address("nonsense"), "nonsense");
    }
}
