#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Notes and Reminders on the watch: the chats app (examples/chats) in notes mode. Notes and reminder lists are chats; lines are cards (kind 3/4: a reminder to do / done, a tap ticks it: t idx); typing adds a line or a reminder; the first row, + New note, starts one. Nothing is kept on the watch.

#[path = "../../pixview/src/rle.rs"]
#[allow(dead_code)]
mod rle;
#[path = "../../chats/src/model.rs"]
#[allow(dead_code)]
mod model;

pub const ASSISTANT: bool = false;
pub const TITLE: &str = "NOTES";
/// An open conversation with nothing in it yet.
pub const EMPTY: &str = "Type to write";
/// Notes: a page (title, plain text, checklists), not chat bubbles; the phone echoes what is typed.
pub const PLAIN: bool = true;
pub const QUICK_TITLE: &str = "ACTIONS";

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../chats/src/ui.rs"]
mod ui;
