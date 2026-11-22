#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[allow(dead_code)]
mod model;
#[allow(dead_code)]
mod player;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod ui;

/// The app's name and mark: examples/spplayer is this app as Spotify (its songs found on Spotify, played from
/// YouTube by the phone).
#[cfg(any(target_os = "none", wp2_sim))]
mod brand {
    pub const NAME: &str = "YouTube Music";
    pub const SPOTIFY: bool = false;
}
