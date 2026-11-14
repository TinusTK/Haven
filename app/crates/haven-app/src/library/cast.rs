//! Casting a stream to a Chromecast instead of playing it here.

use std::{rc::Rc, time::Duration};

use haven_cast::{Cast, Command, Device, Event};
use haven_player::{clock, resume};
use slint::{ModelRc, VecModel};

use super::{Library, PlayRequest, on_ui};
use crate::AppWindow;

/// How long the network is listened to for Chromecasts.
const SEARCH: Duration = Duration::from_secs(4);

impl Library {
    pub(super) fn start_casting(self: &Rc<Self>, ui: &AppWindow) {
        ui.set_cast_target(-1);
        ui.on_find_cast_devices({
            let library = self.clone();
            move || library.find_cast_devices()
        });
        ui.on_cast_command({
            let library = self.clone();
            move |command| library.cast_command(command.as_str())
        });
    }

    /// Listens for Chromecasts and lists them above the streams.
    fn find_cast_devices(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_cast_finding(true);
        ui.set_cast_note("Looking for Chromecasts on your network…".into());
        self.tasks.spawn_blocking(move || {
            let found = haven_cast::discover(SEARCH).map_err(|err| err.to_string());
            on_ui(move |library| library.cast_devices_found(found));
        });
    }

    fn cast_devices_found(&self, found: Result<Vec<Device>, String>) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_cast_finding(false);
        let devices = match found {
            Ok(devices) => devices,
            Err(err) => {
                ui.set_cast_note(format!("Could not look for Chromecasts: {err}.").into());
                return;
            }
        };
        ui.set_cast_note(
            if devices.is_empty() { "No Chromecast answered on this network." } else { "" }.into(),
        );
        let names: Vec<slint::SharedString> =
            devices.iter().map(|device| device.name.as_str().into()).collect();
        ui.set_cast_devices(ModelRc::new(VecModel::from(names)));
        // The chosen device may be gone, or have moved in the list.
        ui.set_cast_target(-1);
        *self.cast_devices.borrow_mut() = devices;
    }

    /// The Chromecast chosen above the stream list, if one is.
    pub(super) fn cast_target(&self) -> Option<Device> {
        let target = self.ui.upgrade()?.get_cast_target();
        self.cast_devices.borrow().get(usize::try_from(target).ok()?).cloned()
    }

    /// Plays a stream on a Chromecast, from where it was left.
    pub(super) fn cast_to(&self, device: Device, request: PlayRequest) {
        let Some(ui) = self.ui.upgrade() else { return };
        let start = self
            .store
            .progress(&request.kind, &request.title_id, &request.video_id)
            .ok()
            .flatten()
            .and_then(|saved| resume::resume_from(saved.position, saved.duration, saved.finished))
            .unwrap_or(0.0);
        ui.set_casting(true);
        ui.set_cast_paused(false);
        ui.set_cast_time("".into());
        ui.set_cast_title(
            if request.detail.is_empty() {
                request.title.clone()
            } else {
                format!("{} · {}", request.title, request.detail)
            }
            .into(),
        );
        ui.set_cast_device_name(device.name.as_str().into());
        ui.set_cast_status(format!("Connecting to {}…", device.name).into());

        let generation = self.cast_generation.get() + 1;
        self.cast_generation.set(generation);
        let what = Cast { address: request.address.clone(), start };
        // Starting a new session drops (and so stops) the one before.
        *self.cast_session.borrow_mut() = Some(haven_cast::cast(device, what, move |event| {
            on_ui(move |library| library.cast_event(generation, event));
        }));
    }

    fn cast_event(&self, generation: u64, event: Event) {
        if generation != self.cast_generation.get() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        match event {
            Event::Started => ui.set_cast_status("".into()),
            Event::Position { position, duration, paused } => {
                ui.set_cast_paused(paused);
                ui.set_cast_time(
                    if duration > 0.0 {
                        format!("{} / {}", clock(position), clock(duration))
                    } else {
                        clock(position)
                    }
                    .into(),
                );
            }
            Event::Ended => {
                *self.cast_session.borrow_mut() = None;
                ui.set_casting(false);
            }
            Event::Failed(reason) => {
                *self.cast_session.borrow_mut() = None;
                ui.set_cast_status(format!("Casting stopped: {reason}.").into());
                ui.set_cast_time("".into());
            }
        }
    }

    /// A button on the casting bar: "pause" (toggles), "back", "forward",
    /// "stop".
    fn cast_command(&self, command: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let session = self.cast_session.borrow();
        let Some(session) = session.as_ref() else {
            // Nothing is playing any more: the bar just closes.
            return ui.set_casting(false);
        };
        match command {
            "pause" => session.send(if ui.get_cast_paused() { Command::Resume } else { Command::Pause }),
            "back" => session.send(Command::Skip(-10.0)),
            "forward" => session.send(Command::Skip(30.0)),
            "stop" => session.send(Command::Stop),
            _ => {}
        }
    }
}
