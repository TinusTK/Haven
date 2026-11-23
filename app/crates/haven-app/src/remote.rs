//! The TV remote's arrows. Slint moves the focus only in one order (Tab
//! and Shift+Tab), so an arrow walks that order until it reaches the
//! nearest thing in its direction: left and right stay on the same row
//! (a shelf), up and down go to the row above or below and pick the thing
//! nearest the column the focus came from.
//!
//! Each focusable thing tells `Remote` where its centre is when it gets
//! the focus (`reports` counts the reports). Standard widgets (switches,
//! menus, text boxes) do not: the walk stops on them, so pages of settings
//! are stepped through in order.

use slint::{ComponentHandle, SharedString, platform::Key, platform::WindowEvent};

use crate::{AppWindow, Remote};

/// Centres closer than this (in logical pixels) are on the same row.
const ROW: f32 = 30.0;
/// The most steps one arrow press takes.
const MOST: usize = 400;

/// Answers the remote's arrows (`Remote.move`) for this window.
pub fn connect(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.global::<Remote>().on_move(move |direction| {
        let weak = weak.clone();
        // Once the key press that asked has been dealt with.
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                step(&ui, direction);
            }
        });
    });
}

/// Where the focused thing is, or None when it did not say.
fn tab(ui: &AppWindow, back: bool) -> Option<(f32, f32)> {
    let remote = ui.global::<Remote>();
    let before = remote.get_reports();
    let key = SharedString::from(if back { Key::Backtab } else { Key::Tab });
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: key });
    (remote.get_reports() != before).then(|| (remote.get_x(), remote.get_y()))
}

/// 0 left, 1 right, 2 up, 3 down.
fn step(ui: &AppWindow, direction: i32) {
    let remote = ui.global::<Remote>();
    let start = (remote.get_x(), remote.get_y());
    let (back, sideways) = match direction {
        0 => (true, true),
        1 => (false, true),
        2 => (true, false),
        _ => (false, false),
    };
    let walk = |ui: &AppWindow| tab(ui, back);
    let undo = |ui: &AppWindow| tab(ui, !back);
    if sideways {
        // The next thing along, if it is on the same row.
        if let Some((_, y)) = walk(ui) {
            if !same_row(y, start.1) {
                undo(ui);
            }
        }
        return;
    }
    let past = |y: f32| if back { y < start.1 - ROW } else { y > start.1 + ROW };
    let mut steps = 0;
    let mut row = None;
    while steps < MOST {
        steps += 1;
        match walk(ui) {
            // Something that does not say where it is: stop on it.
            None => return,
            Some((x, y)) if past(y) => {
                row = Some((x, y));
                break;
            }
            // All the way round: nothing further that way.
            Some((x, y)) if (x - start.0).abs() < 1.0 && (y - start.1).abs() < 1.0 => break,
            Some(_) => {}
        }
    }
    let Some((x, row_y)) = row else {
        for _ in 0..steps {
            undo(ui);
        }
        return;
    };
    // Along that row, the thing nearest the column the focus came from.
    let mut best = (x - start.0).abs();
    loop {
        match walk(ui) {
            Some((x, y)) if same_row(y, row_y) && (x - start.0).abs() < best => best = (x - start.0).abs(),
            _ => {
                undo(ui);
                break;
            }
        }
    }
}

fn same_row(a: f32, b: f32) -> bool {
    (a - b).abs() <= ROW
}
