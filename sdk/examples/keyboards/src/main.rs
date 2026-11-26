#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]
//! Keyboards: text input as any app can have it, from the UI kit (docs/keyboards.md).
//!
//! - QWERTY laid out for the round panel, with the watch's own emoji on a second page (`cmf_wp2_ui::keyboard`,
//!   `cmf_wp2_ui::emoji`)
//! - the rotary keyboard: letters round the rim, the crown picks (`cmf_wp2_ui::dial`)
//! - voice typing: the mic button; the phone writes the words down (`cmf_wp2_ui::dictate`, needs loader 6 and a
//!   phone app that answers the `~` messages, docs/keyboards.md)
//!
//! Send shows what was typed and sends it to the phone as `T text`. The button goes back (and leaves from the
//! keyboard). Nothing is kept.

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
    use cmf_wp2_ui::color::rgb565;
    use cmf_wp2_ui::dial::{self, Dial, Key as DialKey};
    use cmf_wp2_ui::dictate::{Dictate, Out};
    use cmf_wp2_ui::font::{self, centre, wrap};
    use cmf_wp2_ui::haptic::{self, Motor};
    use cmf_wp2_ui::keyboard::{Act, Key, Keyboard, FLASH_MS};
    use cmf_wp2_ui::{emoji, Rgb, Surface, C};
    use core::ptr::addr_of_mut;

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
    const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
    const GREY: u16 = rgb565(0x80, 0x80, 0x80);
    /// A whole emoji page kept at once (18), so turning back to it reads nothing again.
    const EMOJI_RAM: usize = emoji::ram_for(18);
    static mut EMOJI: [u8; EMOJI_RAM] = [0; EMOJI_RAM];

    #[derive(Clone, Copy, PartialEq)]
    enum Screen {
        Keys,
        Dial,
        Voice,
        Sent,
    }

    /// Draws through the Api with body text in Inter.
    #[derive(Clone, Copy)]
    struct Dot(&'static Api);

    impl Surface for Dot {
        fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
            let mut a = self.0;
            Surface::rect(&mut a, x, y, w, h, c);
        }

        fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
            font::draw(self, x, y, s, c);
        }
    }

    struct App {
        api: &'static Api,
        screen: Screen,
        kb: Keyboard,
        dial: Dial,
        dict: Dictate,
        motor: Motor,
        /// a key shown pressed, and until when
        flash: Option<(Key, u32)>,
        /// where voice typing goes back to
        from: Screen,
    }

    impl App {
        fn draw(&mut self) {
            let mut s = Dot(self.api);
            match self.screen {
                Screen::Keys => self.kb.draw(&mut s, "Keyboards"),
                Screen::Dial => {
                    s.rect(0, 0, 466, 466, 0);
                    self.kb.draw_text(&mut s);
                    self.dial.draw(&mut s);
                }
                Screen::Voice => self.dict.draw(&mut s),
                Screen::Sent => {
                    s.rect(0, 0, 466, 466, 0);
                    centre(&mut s, C, 60, "Sent", SOFT);
                    let mut lines = [""; 8];
                    let n = wrap(self.kb.text(), 320, &mut lines).min(7);
                    let y0 = C - n as i32 * 26 / 2;
                    for (i, l) in lines[..n].iter().enumerate() {
                        centre(&mut s, C, y0 + i as i32 * 26, l, 0xFFFF);
                    }
                    centre(&mut s, C, 390, "Tap to type again", GREY);
                }
            }
            self.api.show();
        }

        fn go(&mut self, screen: Screen) {
            self.flash = None;
            self.dial.cancel();
            self.dial.shift = self.kb.shift;
            self.screen = screen;
            self.draw();
        }

        fn send(&mut self) {
            let text = self.kb.text().as_bytes();
            if text.is_empty() {
                return;
            }
            let mut m = [0u8; MSG_MAX];
            let n = text.len().min(MSG_MAX - 1);
            m[0] = b'T';
            m[1..1 + n].copy_from_slice(&text[..n]);
            // no phone: still shown, the demo is about typing
            let _ = self.api.send(&m[..1 + n]);
            self.motor.play(haptic::DOUBLE);
            self.go(Screen::Sent);
        }

        fn voice(&mut self) {
            self.motor.play(haptic::TICK);
            self.from = self.screen;
            self.dict.start(self.api, false);
            self.go(Screen::Voice);
        }

        /// What the QWERTY keyboard asks for.
        fn keys_act(&mut self, a: Act) {
            let mut s = Dot(self.api);
            match a {
                Act::None => {}
                Act::Redraw => {
                    self.motor.play(haptic::TICK);
                    self.flash = None;
                    self.draw();
                }
                Act::Typed(k) => {
                    self.motor.play(haptic::TICK);
                    // a key still shown pressed goes plain first
                    if let Some((old, _)) = self.flash.take() {
                        self.kb.draw_key(&mut s, old, false);
                    }
                    self.kb.draw_text(&mut s);
                    self.kb.draw_key(&mut s, k, true);
                    self.api.show();
                    self.flash = Some((k, self.api.now().wrapping_add(FLASH_MS)));
                }
                Act::Text => {
                    self.motor.play(haptic::TICK);
                    self.kb.draw_text(&mut s);
                    self.api.show();
                }
                Act::Send => self.send(),
                Act::Dial => {
                    self.motor.play(haptic::TICK);
                    self.go(Screen::Dial);
                }
                Act::Voice => self.voice(),
            }
        }

        /// What the rotary keyboard asks for. Its letters come in their case: the QWERTY keyboard's shift is off
        /// for them, and the dial's follows what the text wants next.
        fn dial_act(&mut self, a: dial::Act) {
            let mut s = Dot(self.api);
            match a {
                dial::Act::None => {}
                dial::Act::Show => {
                    self.motor.play(haptic::TICK);
                    self.api.show();
                }
                dial::Act::Redraw => {
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
                dial::Act::Key(k) => {
                    self.motor.play(haptic::TICK);
                    match k {
                        DialKey::Send => return self.send(),
                        DialKey::Del => self.kb.type_key(Key::Del),
                        DialKey::Space => {
                            self.kb.shift = false;
                            self.kb.type_key(Key::Space);
                        }
                        DialKey::Ch(c) => {
                            self.kb.shift = false;
                            self.kb.type_key(Key::Ch(c));
                        }
                    }
                    self.dial.shift = self.kb.shift;
                    self.kb.draw_text(&mut s);
                    self.dial.draw_centre(&mut s);
                    self.api.show();
                }
            }
        }

        /// What the voice typing screen asks for.
        fn voice_out(&mut self, o: Out) {
            match o {
                Out::None => {}
                Out::Redraw => {
                    if self.screen == Screen::Voice {
                        self.draw();
                    }
                }
                Out::Done => {
                    self.kb.add_words(self.dict.text());
                    self.motor.play(haptic::TICK);
                    self.go(self.from);
                }
                Out::Cancel => self.go(self.from),
            }
        }

        fn touch_down(&mut self, x: i32, y: i32) {
            match self.screen {
                Screen::Keys => {
                    let a = self.kb.tap(x, y);
                    self.keys_act(a);
                }
                Screen::Dial => {
                    let a = self.dial.down(&mut Dot(self.api), x, y, self.api.now());
                    self.dial_act(a);
                }
                Screen::Voice => {
                    let o = self.dict.tap(self.api, x, y);
                    self.voice_out(o);
                }
                Screen::Sent => {
                    self.kb.clear();
                    self.go(Screen::Keys);
                }
            }
        }
    }

    fn main(api: &'static Api) -> i32 {
        // the watch's emoji in text and on the keyboard's second page (none on a loader before 6)
        emoji::init(api, unsafe { &mut *addr_of_mut!(EMOJI) });
        let mut kb = Keyboard::new(RED);
        kb.hint = "Type something";
        let mut app = App {
            api,
            screen: Screen::Keys,
            kb,
            dial: Dial::new(),
            dict: Dictate::new(),
            motor: Motor::new(api),
            flash: None,
            from: Screen::Keys,
        };
        app.draw();
        let mut msg = [0u8; MSG_MAX];
        loop {
            let now = api.now();
            let flash = app.flash.map(|(_, t)| t.wrapping_sub(now).min(1000));
            let hold = if app.screen == Screen::Dial { app.dial.hold_wait(now) } else { None };
            let wait = [app.motor.wait(), flash, hold].into_iter().flatten().min().unwrap_or(u32::MAX);
            let e = api.event(app.dict.wait_ms(wait));
            app.motor.update();
            if let Some((k, t)) = app.flash {
                if api.now().wrapping_sub(t) as i32 >= 0 {
                    app.flash = None;
                    if app.screen == Screen::Keys {
                        app.kb.draw_key(&mut Dot(api), k, false);
                        api.show();
                    }
                }
            }
            if app.screen == Screen::Dial && app.dial.tick(&mut Dot(api), api.now()) == dial::Act::Show {
                // held: the letter becomes a capital
                app.motor.play(haptic::TICK);
                api.show();
            }
            if app.dict.active() {
                let o = app.dict.tick(api);
                app.voice_out(o);
            }
            let Some(e) = e else { continue };
            match e.kind {
                TOUCH_DOWN => app.touch_down(e.x.into(), e.y.into()),
                TOUCH_UP if app.screen == Screen::Dial => {
                    let a = app.dial.up(&mut Dot(api), e.x.into(), e.y.into());
                    app.dial_act(a);
                }
                CROWN => match app.screen {
                    Screen::Keys => {
                        let a = app.kb.crown(e.delta.into());
                        app.keys_act(a);
                    }
                    Screen::Dial => {
                        let a = app.dial.crown(&mut Dot(api), e.delta.into(), api.now());
                        app.dial_act(a);
                    }
                    _ => {}
                },
                MESSAGE => {
                    while let Some(m) = api.recv(&mut msg) {
                        if let Some(o) = app.dict.message(m) {
                            app.voice_out(o);
                        }
                    }
                }
                BUTTON => {
                    app.motor.off();
                    match app.screen {
                        Screen::Voice => {
                            let o = app.dict.cancel(api);
                            app.voice_out(o);
                        }
                        Screen::Dial => app.go(Screen::Keys),
                        Screen::Keys | Screen::Sent => return 0,
                    }
                }
                PAUSE => {
                    app.motor.off();
                    // the loader stops the mic while paused: dictation ends with it
                    if app.dict.active() {
                        let o = app.dict.cancel(api);
                        app.voice_out(o);
                    }
                    loop {
                        match api.event(u32::MAX) {
                            Some(e) if e.kind == RESUME => break,
                            Some(e) if e.kind == BUTTON => return 0,
                            _ => {}
                        }
                    }
                    app.draw();
                }
                _ => {}
            }
        }
    }
}
