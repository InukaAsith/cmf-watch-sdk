#![cfg_attr(
    any(target_os = "none", wp2_sim),
    no_main,
    cfg_attr(target_os = "none", no_std)
)]
//! Maps: search, place, route (drive, bike, walk), check the road step by step, navigate (proto.rs).

/// The scene buffer (scene.rs), in the icon's memory (ram.rs).
pub const SCENE_CAP: usize = 26 * 1024;

#[allow(dead_code)]
mod proto;
#[allow(dead_code)]
mod scene;
#[path = "../../nav/src/state.rs"]
#[allow(dead_code)]
mod state;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod render;
/// The turn views' accent (view.rs): a map app's blue.
#[cfg(any(target_os = "none", wp2_sim))]
const ACCENT: cmf_wp2_ui::color::Rgb = cmf_wp2_ui::color::Rgb(0x42, 0x85, 0xF4);
#[cfg(any(target_os = "none", wp2_sim))]
mod ui;
#[cfg(any(target_os = "none", wp2_sim))]
mod ram;
#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../nav/src/view.rs"]
mod view;
