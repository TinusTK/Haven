//! Touch scrolling of a row that scrolls sideways inside a page that
//! scrolls up and down (Haven's shelves), headless. These pass only with
//! the patched Slint core in `app/vendor/i-slint-core` (see its README):
//! with stock Slint 1.18 a quick sideways swipe is lost to the page.

use slint::{
    ComponentHandle, LogicalPosition,
    platform::{PointerEventButton, WindowEvent},
};

slint::slint! {
    export component Page inherits Window {
        width: 400px;
        height: 800px;
        out property <length> page-y: page.content-y;
        out property <length> row-x: row.content-x;
        out property <int> taps;
        page := Flickable {
            // Mouse drags stand in for a finger.
            mouse-drag-pan-enabled: true;
            content-height: 3000px;
            content-y: -500px;
            row := Flickable {
                y: 1000px;
                height: 200px;
                mouse-drag-pan-enabled: true;
                content-width: 3000px;
                TouchArea {
                    x: 0px;
                    y: 0px;
                    width: 100px;
                    height: 200px;
                    clicked => { root.taps += 1; }
                }
            }
        }
    }
}

const ROW_Y: f32 = 500.0 + 100.0; // the row's middle on screen (page at -500)

fn tick(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(ms));
    slint::platform::update_timers_and_animations();
}

/// Presses at `from`, moves through `steps` (16 ms apart, the first one
/// `rest` ms after the press) and lets go. Answers how far the row and the
/// page moved.
fn drag(from: (f32, f32), steps: &[(f32, f32)], rest: u64) -> (f32, f32, i32) {
    let ui = Page::new().unwrap();
    ui.show().unwrap();
    let window = ui.window();
    window.set_size(slint::LogicalSize::new(400.0, 800.0));
    tick(1000);
    let at = |(x, y): (f32, f32)| LogicalPosition::new(x, y);
    let (row0, page0) = (ui.get_row_x(), ui.get_page_y());
    window.dispatch_event(WindowEvent::PointerPressed { position: at(from), button: PointerEventButton::Left });
    tick(rest);
    for step in steps {
        tick(16);
        window.dispatch_event(WindowEvent::PointerMoved { position: at(*step) });
    }
    let last = *steps.last().unwrap_or(&from);
    window.dispatch_event(WindowEvent::PointerReleased { position: at(last), button: PointerEventButton::Left });
    let moved = (row0 - ui.get_row_x(), page0 - ui.get_page_y(), ui.get_taps());
    ui.hide().unwrap();
    moved
}

#[test]
fn rows_and_pages_share_touch_scrolling() {
    i_slint_backend_testing::init_no_event_loop();

    // A slow sideways swipe that drifts down a little: the row follows.
    let (row, page, _) = drag(
        (300.0, ROW_Y),
        &[(290.0, ROW_Y + 1.0), (270.0, ROW_Y + 3.0), (250.0, ROW_Y + 6.0), (230.0, ROW_Y + 9.0), (210.0, ROW_Y + 12.0), (190.0, ROW_Y + 15.0)],
        120,
    );
    assert!(row >= 100.0, "slow swipe: the row moved {row}");
    assert_eq!(page, 0.0, "slow swipe: the page moved");

    // A quick swipe whose first sample is 18 px sideways and 10 px down,
    // moving at once (no rest after the press): the row takes it.
    let (row, page, _) = drag(
        (300.0, ROW_Y),
        &[(282.0, ROW_Y + 10.0), (255.0, ROW_Y + 14.0), (225.0, ROW_Y + 17.0), (195.0, ROW_Y + 19.0), (165.0, ROW_Y + 20.0)],
        0,
    );
    assert!(row >= 100.0, "quick swipe: the row moved {row}");
    assert_eq!(page, 0.0, "quick swipe: the page moved");

    // Up and down still scrolls the page, also when starting on the row.
    let (row, page, _) = drag((200.0, ROW_Y), &[(201.0, ROW_Y - 20.0), (202.0, ROW_Y - 60.0), (203.0, ROW_Y - 100.0)], 0);
    assert!(page >= 80.0, "vertical swipe: the page moved {page}");
    assert_eq!(row, 0.0, "vertical swipe: the row moved");

    // A tap on something in the row still counts as a tap.
    let (_, _, taps) = drag((50.0, ROW_Y), &[], 90);
    assert_eq!(taps, 1, "a tap in the row");
    let (_, _, taps) = drag((50.0, ROW_Y), &[], 0);
    assert_eq!(taps, 1, "a quick tap in the row");
}
