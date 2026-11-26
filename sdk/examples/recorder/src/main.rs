//! Recorder: YouTube Music's app (examples/ytmusic, the same sources) as a voice recorder. A recording is voice
//! typing the phone keeps (cmf_wp2_ui::dictate, `~3`): saved on the iPhone by CMFX with its words. Recordings
//! lists them, and one plays as a song does: streamed by the phone into the player's ~7 s buffer.
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
    pub const NAME: &str = "Recorder";
    pub const SPOTIFY: bool = false;
    pub const RECORDER: bool = true;
    pub const ACCENT: cmf_wp2_ui::color::Rgb = cmf_wp2_ui::color::Rgb(0xF0, 0x2B, 0x3A);
    pub const INK: u16 = 0xFFFF;
}
