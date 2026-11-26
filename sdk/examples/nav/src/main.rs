#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

/// The scene buffer (scene.rs).
pub const SCENE_CAP: usize = 30 * 1024;

#[allow(dead_code)]
mod state;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

/// The turn views' accent (view.rs): red.
#[cfg(any(target_os = "none", wp2_sim))]
const ACCENT: cmf_wp2_ui::color::Rgb = cmf_wp2_ui::color::Rgb(0xD7, 0x19, 0x21);
#[cfg(any(target_os = "none", wp2_sim))]
mod ui;
#[cfg(any(target_os = "none", wp2_sim))]
mod view;
#[cfg(any(target_os = "none", wp2_sim))]
mod start;
// the map view's streets: Maps' scene and drawing (examples/atlas), the phone sends them as Maps gets them
#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../atlas/src/scene.rs"]
#[allow(dead_code)]
mod scene;
#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../atlas/src/render.rs"]
#[allow(dead_code)]
mod render;
#[cfg(any(target_os = "none", wp2_sim))]
mod ram;
