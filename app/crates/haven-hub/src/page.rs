//! The status page at the Hub's address: the pairing code, the paired
//! devices, and every setting the Hub keeps: media servers, add-ons and
//! Live TV lists (switch on or off, remove, add), the keys, and the Trakt
//! and Simkl links.

use crate::hub::{Hub, now_ms};

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The On/Off and Remove buttons of one row. `field` names the thing
/// (`address` or `id`), `value` is its address or number.
fn controls(base: &str, field: &str, value: &str, on: bool) -> String {
    let value = escape(value);
    format!(
        "<span class=controls>\
         <form method=post action=/{base}/switch><input type=hidden name={field} value=\"{value}\">\
         <input type=hidden name=on value={next}><button class=\"switch{state}\">{label}</button></form>\
         <form method=post action=/{base}/remove><input type=hidden name={field} value=\"{value}\">\
         <button class=quiet>Remove</button></form></span>",
        next = if on { 0 } else { 1 },
        state = if on { " on" } else { "" },
        label = if on { "On" } else { "Off" },
    )
}

fn ago(ms: i64) -> String {
    let minutes = (now_ms() - ms).max(0) / 60_000;
    match minutes {
        0 => "just now".to_owned(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

pub fn status(hub: &Hub) -> String {
    let devices = hub.devices();
    let device_rows: String = if devices.is_empty() {
        "<p class=dim>No devices yet. Pair one with the code above.</p>".to_owned()
    } else {
        devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                format!(
                    "<div class=row><span>{} <span class=dim>· paired {}</span></span>\
                     <form method=post action=/devices/forget><input type=hidden name=index value={index}>\
                     <button>Forget</button></form></div>",
                    escape(&device.name),
                    ago(device.paired_at)
                )
            })
            .collect()
    };
    let installed = hub.installed();
    let addon_rows: String = if installed.is_empty() {
        "<p class=dim>None yet.</p>".to_owned()
    } else {
        installed
            .iter()
            .map(|(url, manifest)| {
                format!(
                    "<div class=row><span>{} <span class=dim>{}</span></span>{}</div>",
                    escape(&manifest.name),
                    escape(&manifest.version),
                    controls("addons", "address", url, hub.addon_on(url))
                )
            })
            .collect()
    };
    let servers = hub.server_rows();
    let server_rows: String = if servers.is_empty() {
        "<p class=dim>None yet.</p>".to_owned()
    } else {
        servers
            .iter()
            .map(|row| {
                format!(
                    "<div class=row><span>{} <span class=dim>{}</span></span>{}</div>",
                    escape(&row.name),
                    escape(&row.detail),
                    controls("servers", "id", &row.id.to_string(), row.on)
                )
            })
            .collect()
    };
    let lists = hub.tv_rows();
    let tv_rows: String = if lists.is_empty() {
        "<p class=dim>None yet.</p>".to_owned()
    } else {
        lists
            .iter()
            .map(|row| {
                format!(
                    "<div class=row><span>{} <span class=dim>{}</span></span>{}</div>",
                    escape(&row.name),
                    escape(&row.id),
                    controls("tv", "address", &row.id, row.on)
                )
            })
            .collect()
    };
    let set = hub.keys_set();
    let key_box = |name: &str, label: &str, is_set: bool| {
        format!(
            "<label>{label} <span class=dim>{}</span>\
             <input type=password name={name} autocomplete=off placeholder=\"{}\"></label>",
            if is_set { "· set" } else { "· not set" },
            if is_set { "Type a new one to replace it" } else { "Paste it here" }
        )
    };
    let keys = format!(
        "<form class=keys method=post action=/keys>{}{}{}{}{}<button>Save keys</button></form>\
         <p class=dim>A box left empty keeps its key. The keys are never shown here.</p>",
        key_box("tmdb", "TMDB", set.tmdb),
        key_box("omdb", "OMDb", set.omdb),
        key_box("trakt_id", "Trakt Client ID", set.trakt_id),
        key_box("trakt_secret", "Trakt Client Secret", set.trakt_secret),
        key_box("simkl_id", "Simkl Client ID", set.simkl_id),
    );
    let report = hub.background.report.lock().unwrap().clone();
    let when = |at: i64| if at == 0 { "not yet".to_owned() } else { ago(at * 1000) };
    let tracking = if report.linked.is_empty() {
        "<p class=dim>No Trakt or Simkl account yet. Link one in Haven on any device, then press Send everything to the Hub.</p>".to_owned()
    } else {
        let unlink: String = report
            .linked
            .iter()
            .map(|service| {
                format!(
                    "<div class=row><span>{0}</span><form method=post action=/tracking/unlink>\
                     <input type=hidden name=service value=\"{0}\"><button class=quiet>Unlink</button></form></div>",
                    escape(service)
                )
            })
            .collect();
        format!(
            "<p>Keeping {} in step: last time {}.{}</p>{unlink}",
            escape(&report.linked.join(" and ")),
            when(report.last_sync),
            if report.sync_problem.is_empty() {
                String::new()
            } else {
                format!(" <span class=dim>Last problem: {}</span>", escape(&report.sync_problem))
            }
        )
    };
    let upcoming: String = if report.upcoming.is_empty() {
        "<p class=dim>Nothing airing this week from the series you follow.</p>".to_owned()
    } else {
        report
            .upcoming
            .iter()
            .map(|(day, series, which)| {
                format!("<div class=row><span>{} <span class=dim>{}</span></span><span class=dim>{}</span></div>", escape(series), escape(which), escape(day))
            })
            .collect()
    };
    let episodes = format!(
        "<p class=dim>{} series followed · checked {}</p>{upcoming}",
        report.series_checked,
        when(report.last_episodes)
    );
    format!(
        r#"<!doctype html>
<html lang=en><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>Haven Hub</title>
<style>
:root{{color-scheme:light dark;--bg:#f6f5f2;--card:#fff;--text:#1b1b1f;--dim:#6b6b73;--accent:#5b5bd6;--border:#e3e2de}}
@media (prefers-color-scheme:dark){{:root{{--bg:#111114;--card:#1b1b20;--text:#ededf0;--dim:#9a9aa3;--border:#2a2a31}}}}
body{{margin:0;background:var(--bg);color:var(--text);font:16px/1.5 system-ui,sans-serif}}
main{{max-width:640px;margin:0 auto;padding:24px 16px}}
h1{{margin:0 0 4px;font-size:26px}} h2{{font-size:18px;margin:28px 0 8px}}
.card{{background:var(--card);border:1px solid var(--border);border-radius:12px;padding:16px}}
.code{{font-size:44px;font-weight:700;letter-spacing:.12em;font-variant-numeric:tabular-nums}}
.dim{{color:var(--dim);font-size:14px}}
.row{{display:flex;justify-content:space-between;align-items:center;gap:12px;padding:8px 0;border-top:1px solid var(--border)}}
.row:first-child{{border-top:0}}
form{{margin:0}} input[type=url],input[type=text],input[type=password]{{flex:1;min-width:0;padding:8px 10px;border-radius:8px;border:1px solid var(--border);background:var(--bg);color:var(--text);font:inherit}}
button{{padding:8px 14px;border-radius:8px;border:0;background:var(--accent);color:#fff;font-weight:600;cursor:pointer;font:inherit}}
button.quiet{{background:transparent;color:var(--dim);border:1px solid var(--border)}}
button.switch{{background:var(--border);color:var(--dim);min-width:56px}} button.switch.on{{background:var(--accent);color:#fff}}
.controls{{display:flex;gap:6px;flex-shrink:0}}
.row>span:first-child{{min-width:0;overflow-wrap:anywhere}}
.add{{display:flex;gap:8px;margin-top:12px;flex-wrap:wrap}}
.keys{{display:grid;gap:10px}} .keys label{{display:grid;gap:4px}} .keys button{{justify-self:start}}
</style></head><body><main>
<h1>Haven Hub</h1>
<p class=dim>{name} · version {version}</p>
<h2>Pair a device</h2>
<div class=card>
<div class=code>{code}</div>
<p class=dim>In Haven, open Settings › Media servers, choose Haven Hub, type this Hub's address and this code. The code changes after each device pairs.</p>
</div>
<h2>Paired devices</h2>
<div class=card>{device_rows}</div>
<h2 id=tracking>Trakt and Simkl</h2>
<div class=card>{tracking}</div>
<h2>New episodes this week</h2>
<div class=card>{episodes}</div>
<h2>Settings</h2>
<p class=dim>Change things here, or set them up in Haven on a computer and press Send everything to the Hub there: the Hub then matches that device. Off means the Hub keeps it but does not use it, and a device that gets the Hub's setup gets it switched off.</p>
<h2 id=servers>Media servers</h2>
<div class=card>{server_rows}
<p class=dim>To add a server, sign in to it in Haven on any device, then press Send everything to the Hub.</p>
</div>
<h2 id=addons>Add-ons</h2>
<div class=card>{addon_rows}
<form class=add method=post action=/addons/add><input type=url name=address placeholder="Add-on address" required><button>Add</button></form>
</div>
<h2 id=tv>Live TV lists</h2>
<div class=card>{tv_rows}
<form class=add method=post action=/tv/add><input type=text name=name placeholder="Name"><input type=url name=address placeholder="Playlist address (M3U)" required><button>Add</button></form>
</div>
<h2 id=keys>Keys</h2>
<div class=card>{keys}</div>
</main></body></html>"#,
        name = escape(&hub.name),
        version = env!("CARGO_PKG_VERSION"),
        code = hub.pairing_code(),
    )
}

#[cfg(test)]
mod tests {
    use haven_core::storage::{Store, TvSource};

    use super::*;

    #[test]
    fn the_page_shows_every_setting_without_the_keys() {
        let hub = Hub::new(Store::in_memory().unwrap(), haven_core::http::client().unwrap(), "Den".into());
        hub.save_keys(&haven_core::settings::Keys { tmdb: "secret-key".into(), ..Default::default() });
        hub.store
            .lock()
            .unwrap()
            .save_tv_source(&TvSource { url: "http://tv/a.m3u".into(), name: "<News>".into(), playlist: String::new() })
            .unwrap();
        let page = status(&hub);
        for part in ["id=servers", "id=addons", "id=tv", "id=keys", "action=/tv/switch", "&lt;News&gt;"] {
            assert!(page.contains(part), "{part}");
        }
        assert!(!page.contains("secret-key"));
    }
}
