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
/// WhatsApp's voice notes: recorded and sent, played on the speaker (voice.rs).
pub const VOICE: bool = false;
/// Tabs under the title that filter the list (`f tab` to the phone).
pub const TABS: &[&str] = &[];
pub const QUICK_TITLE: &str = "Reply";
/// The name at the top, beside the app's icon.
pub const NAME: &str = "Mail";
/// The app's colours: its accent (buttons, badges, the heading's dot), our bubbles, theirs.
pub const ACCENT: u32 = 0xE8453C;
pub const MINE: u32 = 0x5A1D1A;
pub const THEIRS: u32 = 0x1E1E22;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
#[path = "../../chats/src/ui.rs"]
mod ui;
