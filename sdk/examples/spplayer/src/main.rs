//! Spotify on the watch's speaker: YouTube Music's app (examples/ytmusic, the same sources) under Spotify's name
//! and mark. The phone (CMFX) searches Spotify's catalog and plays each song from YouTube: Spotify withholds the
//! audio of its own streams from third-party players.
#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[allow(dead_code)]
#[path = "../../ytmusic/src/model.rs"]
mod model;
#[allow(dead_code)]
#[path = "../../ytmusic/src/player.rs"]
mod player;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../ytmusic/src/ui.rs"]
mod ui;

#[cfg(any(target_os = "none", wp2_sim))]
mod brand {
    pub const NAME: &str = "Spotify";
    pub const SPOTIFY: bool = true;
    pub const RECORDER: bool = false;
    pub const ACCENT: cmf_wp2_ui::color::Rgb = cmf_wp2_ui::color::Rgb(0x1E, 0xD7, 0x60);
    pub const INK: u16 = 0;
}
