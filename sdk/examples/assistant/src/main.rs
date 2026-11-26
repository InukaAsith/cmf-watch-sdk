#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Gemini on the watch: the chats app (examples/chats) in one-conversation mode. The phone asks Gemini and streams
//! the answer back with `T` messages (examples/chats/src/model.rs).

#[path = "../../pixview/src/rle.rs"]
#[allow(dead_code)]
mod rle;
#[path = "../../chats/src/model.rs"]
#[allow(dead_code)]
mod model;

pub const ASSISTANT: bool = true;
pub const TITLE: &str = "GEMINI";
/// An open conversation with nothing in it yet.
pub const EMPTY: &str = "Ask me anything";
/// Notes: a page (title, plain text, checklists), not chat bubbles; the phone echoes what is typed.
pub const PLAIN: bool = false;
pub const QUICK_TITLE: &str = "ASK";

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../chats/src/ui.rs"]
mod ui;
