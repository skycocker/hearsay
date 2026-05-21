//! System tray for hearsayd. Mirrors the pattern from shape-emg —
//! pure-Rust `tray-icon` crate, runs in-process. On macOS, the tray loop
//! owns the main thread (AppKit requirement) and the server runs on a
//! tokio worker task; on Linux, GTK owns its own thread.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::state::AppState;

/// Synthesize a 32×32 RGBA icon at startup — saves shipping an asset.
/// Black background with a white "h" silhouette.
fn make_icon() -> Icon {
    const N: usize = 32;
    let mut rgba = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let i = (y * N + x) * 4;
            // Default: black square.
            rgba[i] = 0;
            rgba[i + 1] = 0;
            rgba[i + 2] = 0;
            rgba[i + 3] = 255;

            // White "h" shape: two verticals + a crossbar.
            let in_left_bar = (8..=10).contains(&x) && (6..=26).contains(&y);
            let in_right_bar = (21..=23).contains(&x) && (15..=26).contains(&y);
            let in_crossbar = (15..=17).contains(&y) && (8..=23).contains(&x);
            if in_left_bar || in_right_bar || in_crossbar {
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
            }
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("build icon")
}

/// Runs the tray event loop. Blocks forever — invoke on the main thread on
/// macOS (NSApplication requires it) and on a dedicated thread on Linux.
pub fn run(port: u16, state: Arc<AppState>) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().expect("tray must run on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    let url = format!("http://127.0.0.1:{port}");

    let item_open = MenuItem::new(format!("Open hearsay: {url}"), true, None);
    let item_status = MenuItem::new("Active sessions: 0", false, None);
    let item_quit = MenuItem::new("Quit hearsay", true, None);

    let open_id = item_open.id().clone();
    let quit_id = item_quit.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&item_open);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&item_status);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&item_quit);

    // Keep alive for the lifetime of the loop — TrayIcon drops the icon
    // from the menu bar on drop.
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(format!("hearsay (port {port})"))
        .with_icon(make_icon())
        .build()
        .expect("build tray icon");

    let receiver = MenuEvent::receiver();
    let last_status: Arc<Mutex<String>> = Arc::new(Mutex::new(String::from("Active sessions: 0")));

    loop {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration();
        }
        #[cfg(target_os = "macos")]
        {
            // SAFETY: these AppKit calls are only valid on the main thread,
            // which we verified above via MainThreadMarker. Pumping events
            // through `sendEvent` is the standard idiom — see Apple's
            // "Manual Event Processing" docs.
            #[allow(unsafe_code)]
            unsafe {
                use objc2_app_kit::NSApplication;
                use objc2_foundation::{MainThreadMarker, NSDate, NSDefaultRunLoopMode};
                let mtm = MainThreadMarker::new().unwrap();
                let app = NSApplication::sharedApplication(mtm);
                while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    objc2_app_kit::NSEventMask::Any,
                    Some(&NSDate::distantPast()),
                    NSDefaultRunLoopMode,
                    true,
                ) {
                    app.sendEvent(&event);
                }
            }
        }

        if let Ok(event) = receiver.recv_timeout(Duration::from_millis(100)) {
            if event.id == open_id {
                let _ = open::that(&url);
            } else if event.id == quit_id {
                std::process::exit(0);
            }
        }

        // Refresh the "Active sessions: N" line, same thread as the menu
        // item so we don't need Send for tray_icon::MenuItem.
        let n = state.sessions.active_ids().len();
        let new_text = format!("Active sessions: {n}");
        let mut current = last_status.lock();
        if *current != new_text {
            item_status.set_text(&new_text);
            *current = new_text;
        }
    }
}
