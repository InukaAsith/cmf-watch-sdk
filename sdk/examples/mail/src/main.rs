#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Gmail on the watch: the chats app (examples/chats) in mail mode. Threads are chats (sender, subject), the email's text arrives as cards; replies are typed or quick; quick replies starting with • are actions (Archive, Mark unread). Nothing is kept on the watch.

#[path = "../../pixview/src/rle.rs"]
#[allow(dead_code)]
mod rle;
#[path = "../../chats/src/model.rs"]
#[allow(dead_code)]
mod model;

pub const ASSISTANT: bool = false;
pub const TITLE: &str = "MAIL";
/// An open conversation with nothing in it yet.
pub const EMPTY: &str = "Loading";
/// Notes: a page (title, plain text, checklists), not chat bubbles; the phone echoes what is typed.
pub const PLAIN: bool = false;
pub const QUICK_TITLE: &str = "REPLY";

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../chats/src/ui.rs"]
mod ui;
