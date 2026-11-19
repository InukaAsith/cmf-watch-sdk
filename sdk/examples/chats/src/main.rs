#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[path = "../../pixview/src/rle.rs"]
#[allow(dead_code)]
mod rle;
#[allow(dead_code)]
mod model;

/// The same app as the Gemini assistant (examples/assistant): one conversation, no list.
pub const ASSISTANT: bool = false;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod ui;
