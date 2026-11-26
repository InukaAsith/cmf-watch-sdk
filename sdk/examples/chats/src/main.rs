#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[path = "../../pixview/src/rle.rs"]
#[allow(dead_code)]
mod rle;
#[allow(dead_code)]
mod model;

/// The same app as the Gemini assistant (examples/assistant): one conversation, no list.
pub const ASSISTANT: bool = false;
pub const TITLE: &str = "WHATSAPP";
/// An open conversation with nothing in it yet.
pub const EMPTY: &str = "Loading";
/// Notes: a page (title, plain text, checklists), not chat bubbles; the phone echoes what is typed.
pub const PLAIN: bool = false;
/// WhatsApp's voice notes: recorded and sent, played on the speaker (voice.rs).
pub const VOICE: bool = true;
/// Tabs under the title that filter the list (`f tab` to the phone).
/// A list row held: delete it (after a confirm)
pub const DELETE: bool = false;
pub const TABS: &[&str] = &[];
pub const QUICK_TITLE: &str = "Reply";
/// The name at the top, beside the app's icon.
pub const NAME: &str = "WhatsApp";
/// The app's colours: its accent (buttons, badges, the heading's dot), our bubbles, theirs.
pub const ACCENT: u32 = 0x25D366;
pub const MINE: u32 = 0x005C4B;
pub const THEIRS: u32 = 0x1F2C34;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod ui;
