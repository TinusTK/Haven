//! Casting: finding Chromecasts on the local network and playing a web
//! address on one, with pause, seek and stop.
//!
//! Finding uses mDNS (`_googlecast._tcp`); playing uses the Cast protocol's
//! default media receiver, which fetches the address itself. So only
//! addresses the Chromecast can reach will play, in formats it knows
//! (MP4, WebM, HLS, DASH).
//!
//! Everything here blocks, so call it off the UI thread.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use rust_cast::{
    CastDevice,
    channels::{
        media::{LoadOptions, Media, PlayerState, StatusEntry, StreamType},
        receiver::CastDeviceApp,
    },
};

const SERVICE: &str = "_googlecast._tcp.local.";
const RECEIVER: &str = "receiver-0";
/// How often the Chromecast is asked where playback stands.
const TICK: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not search the network: {0}")]
    Search(String),
    #[error("could not talk to the device: {0}")]
    Device(String),
}

/// A Chromecast (or other Cast device) on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The name its owner gave it ("Living Room TV").
    pub name: String,
    pub host: String,
    pub port: u16,
}

/// Listens for Cast devices for `wait` and returns those that answered,
/// by name.
pub fn discover(wait: Duration) -> Result<Vec<Device>, Error> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|err| Error::Search(err.to_string()))?;
    let found = daemon.browse(SERVICE).map_err(|err| Error::Search(err.to_string()))?;
    let until = Instant::now() + wait;
    let mut devices: Vec<Device> = Vec::new();
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Ok(event) = found.recv_timeout(left) else { break };
        let mdns_sd::ServiceEvent::ServiceResolved(service) = event else { continue };
        let Some(address) = service.get_addresses_v4().into_iter().next() else { continue };
        let name = service
            .get_property_val_str("fn")
            .map(str::to_owned)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| service.get_hostname().trim_end_matches('.').to_owned());
        let device = Device { name, host: address.to_string(), port: service.get_port() };
        if !devices.iter().any(|known| known.host == device.host && known.port == device.port) {
            devices.push(device);
        }
    }
    let _ = daemon.shutdown();
    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(devices)
}

/// The media type a Chromecast is told to expect, from the address.
pub fn content_type(address: &str) -> &'static str {
    let path = address.split(['?', '#']).next().unwrap_or(address).to_ascii_lowercase();
    match path.rsplit('.').next().unwrap_or_default() {
        "m3u8" => "application/x-mpegURL",
        "mpd" => "application/dash+xml",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "aac" | "m4a" => "audio/mp4",
        _ => "video/mp4",
    }
}

/// What to play.
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub address: String,
    /// Seconds in to start at.
    pub start: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Pause,
    Resume,
    /// Jump by this many seconds, forwards or back.
    Skip(f64),
    Stop,
}

/// News from a casting session.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The Chromecast accepted the address.
    Started,
    Position { position: f64, duration: f64, paused: bool },
    /// Playback is over (finished, stopped here, or stopped on the device).
    Ended,
    Failed(String),
}

/// A casting session in flight: send it commands; dropping it stops it.
pub struct Session {
    commands: mpsc::Sender<Command>,
}

impl Session {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

/// Starts playing on a device, on a thread of its own. `on_event` is
/// called from that thread.
pub fn cast(device: Device, what: Cast, on_event: impl Fn(Event) + Send + 'static) -> Session {
    let (commands, inbox) = mpsc::channel();
    std::thread::Builder::new()
        .name("haven-cast".into())
        .spawn(move || {
            if let Err(err) = run(&device, &what, &inbox, &on_event) {
                on_event(Event::Failed(err.to_string()));
            } else {
                on_event(Event::Ended);
            }
        })
        .expect("could not start the casting thread");
    Session { commands }
}

fn device_error(err: rust_cast::errors::Error) -> Error {
    Error::Device(err.to_string())
}

fn run(
    device: &Device,
    what: &Cast,
    inbox: &mpsc::Receiver<Command>,
    on_event: &dyn Fn(Event),
) -> Result<(), Error> {
    // Chromecasts present a certificate of their own making, so the host
    // cannot be verified; the link is still encrypted.
    let cast = CastDevice::connect_without_host_verification(device.host.clone(), device.port)
        .map_err(device_error)?;
    cast.connection.connect(RECEIVER).map_err(device_error)?;
    cast.heartbeat.ping().map_err(device_error)?;
    let app = cast.receiver.launch_app(&CastDeviceApp::DefaultMediaReceiver).map_err(device_error)?;
    cast.connection.connect(app.transport_id.as_str()).map_err(device_error)?;
    let loaded = cast
        .media
        .load_with_opts(
            app.transport_id.as_str(),
            app.session_id.as_str(),
            &Media {
                content_id: what.address.clone(),
                stream_type: StreamType::Buffered,
                content_type: content_type(&what.address).to_owned(),
                metadata: None,
                duration: None,
            },
            LoadOptions { current_time: what.start.max(0.0), autoplay: true },
        )
        .map_err(device_error)?;
    let Some(mut entry) = loaded.entries.into_iter().next() else {
        return Err(Error::Device("the device did not start playing".to_owned()));
    };
    on_event(Event::Started);

    loop {
        let session = entry.media_session_id;
        let transport = app.transport_id.as_str();
        match inbox.recv_timeout(TICK) {
            Ok(Command::Pause) => entry = cast.media.pause(transport, session).map_err(device_error)?,
            Ok(Command::Resume) => entry = cast.media.play(transport, session).map_err(device_error)?,
            Ok(Command::Skip(seconds)) => {
                let target = (entry.current_time.unwrap_or(0.0) as f64 + seconds).max(0.0);
                entry = cast
                    .media
                    .seek(transport, session, Some(target as f32), None)
                    .map_err(device_error)?;
            }
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = cast.media.stop(transport, session);
                let _ = cast.receiver.stop_app(app.session_id.as_str());
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Keep the link alive, then ask where playback stands.
                cast.heartbeat.ping().map_err(device_error)?;
                let _ = cast.heartbeat.pong();
                let status = cast.media.get_status(transport, Some(session)).map_err(device_error)?;
                match status.entries.into_iter().next() {
                    Some(current) => entry = current,
                    // No media session any more: it finished or was stopped.
                    None => return Ok(()),
                }
            }
        }
        if entry.player_state == PlayerState::Idle && entry.idle_reason.is_some() {
            return Ok(());
        }
        on_event(position(&entry));
    }
}

fn position(entry: &StatusEntry) -> Event {
    Event::Position {
        position: entry.current_time.unwrap_or(0.0) as f64,
        duration: entry.media.as_ref().and_then(|media| media.duration).unwrap_or(0.0) as f64,
        paused: entry.player_state == PlayerState::Paused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_follow_the_address() {
        assert_eq!(content_type("https://x/a/master.m3u8?token=1"), "application/x-mpegURL");
        assert_eq!(content_type("https://x/film.MKV"), "video/x-matroska");
        assert_eq!(content_type("https://x/film.webm#t=3"), "video/webm");
        assert_eq!(content_type("https://x/Videos/abc/stream?static=true"), "video/mp4");
        assert_eq!(content_type("https://x/radio.mp3"), "audio/mpeg");
    }

    #[test]
    fn dropping_a_session_asks_it_to_stop() {
        let (commands, inbox) = mpsc::channel();
        drop(Session { commands });
        assert_eq!(inbox.recv().unwrap(), Command::Stop);
    }
}
