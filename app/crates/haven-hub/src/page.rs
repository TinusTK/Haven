//! The status page at the Hub's address: the pairing code, the paired
//! devices, and the add-ons, with a box to add one.

use crate::hub::{Hub, now_ms};

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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
                    "<div class=row><span>{} <span class=dim>{}</span></span>\
                     <form method=post action=/addons/remove><input type=hidden name=address value=\"{}\">\
                     <button>Remove</button></form></div>",
                    escape(&manifest.name),
                    escape(&manifest.version),
                    escape(url)
                )
            })
            .collect()
    };
    let report = hub.background.report.lock().unwrap().clone();
    let when = |at: i64| if at == 0 { "not yet".to_owned() } else { ago(at * 1000) };
    let tracking = if report.linked.is_empty() {
        "<p class=dim>No Trakt or Simkl account yet. Link one in Haven on any device, then press Send everything to the Hub.</p>".to_owned()
    } else {
        format!(
            "<p>Keeping {} in step: last time {}.{}</p>",
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
form{{margin:0}} input[type=url]{{flex:1;min-width:0;padding:8px 10px;border-radius:8px;border:1px solid var(--border);background:var(--bg);color:var(--text)}}
button{{padding:8px 14px;border-radius:8px;border:0;background:var(--accent);color:#fff;font-weight:600;cursor:pointer}}
.add{{display:flex;gap:8px;margin-top:12px}}
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
<h2>Trakt and Simkl</h2>
<div class=card>{tracking}</div>
<h2>New episodes this week</h2>
<div class=card>{episodes}</div>
<h2>Add-ons</h2>
<div class=card>{addon_rows}
<form class=add method=post action=/addons/add><input type=url name=address placeholder="Add-on address" required><button>Add</button></form>
</div>
</main></body></html>"#,
        name = escape(&hub.name),
        version = env!("CARGO_PKG_VERSION"),
        code = hub.pairing_code(),
    )
}
