#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Browser: the web through the phone, read on the watch (ui.rs).

#[path = "../../pixview/src/rle.rs"]
#[allow(dead_code)]
mod rle;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod keys;
#[cfg(any(target_os = "none", wp2_sim))]
mod long;
#[cfg(any(target_os = "none", wp2_sim))]
mod page;
#[cfg(any(target_os = "none", wp2_sim))]
mod ui;
