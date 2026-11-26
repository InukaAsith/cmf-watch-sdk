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
/// WhatsApp's voice notes: recorded and sent, played on the speaker (voice.rs).
pub const VOICE: bool = false;
/// Tabs under the title that filter the list (`f tab` to the phone).
/// A list row held: delete it (after a confirm)
pub const DELETE: bool = true;
pub const TABS: &[&str] = &["Notes", "To-do", "Reminders"];
pub const QUICK_TITLE: &str = "Actions";
/// The name at the top, beside the app's icon.
pub const NAME: &str = "Notes";
/// The app's colours: its accent (buttons, badges, the heading's dot), our bubbles, theirs.
pub const ACCENT: u32 = 0xF5C518;
pub const MINE: u32 = 0x4A3B05;
pub const THEIRS: u32 = 0x1C1C1C;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../chats/src/ui.rs"]
mod ui;
