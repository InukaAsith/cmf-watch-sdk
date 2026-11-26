#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Offline Maps: a map saved on the watch and drawn by it (pack.rs), the watch's own GPS, markers, a route from the
//! phone followed without it, a track of where you went and the way back to its start.

/// The scene buffer (scene.rs).
pub const SCENE_CAP: usize = 16 * 1024;

#[allow(dead_code)]
mod geo;
#[allow(dead_code)]
mod marks;
#[allow(dead_code)]
mod pack;
#[path = "../../atlas/src/scene.rs"]
#[allow(dead_code)]
mod scene;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../atlas/src/render.rs"]
#[allow(dead_code)]
mod render;
#[cfg(any(target_os = "none", wp2_sim))]
mod ram;
#[cfg(any(target_os = "none", wp2_sim))]
mod ui;
// turn by turn: Navigation's state and views (examples/nav), in Maps' blue
#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../nav/src/state.rs"]
#[allow(dead_code)]
mod state;
#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../nav/src/view.rs"]
#[allow(dead_code)]
mod view;
#[cfg(any(target_os = "none", wp2_sim))]
const ACCENT: cmf_wp2_ui::color::Rgb = cmf_wp2_ui::color::Rgb(0x42, 0x85, 0xF4);
