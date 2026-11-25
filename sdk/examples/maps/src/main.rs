#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Offline Maps: a map saved on the watch and drawn by it (pack.rs), the watch's own GPS, markers, a route from the
//! phone followed without it, a track of where you went and the way back to its start.

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
