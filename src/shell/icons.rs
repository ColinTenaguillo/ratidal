//! The two icon sets, and which one is in use.
//!
//! Plain Unicode by default: nerd-font glyphs show as empty boxes for anyone
//! without one of those fonts, and an app that opens full of blank squares
//! looks broken rather than unconfigured. Turned on in Settings by someone
//! who has the font.
//!
//! One place, so a change of icon lands everywhere at once and the two sets
//! cannot drift apart.

use std::sync::atomic::{AtomicBool, Ordering};

/// Read on every draw, so it is a global rather than threaded through every
/// render call: the icons are leaves of the tree and passing a config down
/// to each of them is a change to every signature in the shell.
///
/// Set by hand rather than detected. A terminal does not say what font it
/// is using, and the glyphs cannot be measured either: nerd fonts set the
/// x-advance of every glyph to one cell, which is also what a font without
/// them advances when it draws a replacement box. lazygit, starship and
/// yazi all ask rather than guess.
static NERD_FONT: AtomicBool = AtomicBool::new(false);

pub fn set_nerd_font(on: bool) {
    NERD_FONT.store(on, Ordering::Relaxed);
}

pub fn nerd_font() -> bool {
    NERD_FONT.load(Ordering::Relaxed)
}

/// Pick between the plain glyph and the nerd-font one.
fn pick(plain: &'static str, nerd: &'static str) -> &'static str {
    if nerd_font() {
        nerd
    } else {
        plain
    }
}

pub fn music() -> &'static str {
    pick("♫", "\u{f001}")
}
pub fn explore() -> &'static str {
    pick("⊕", "\u{f002}")
}
pub fn feed() -> &'static str {
    pick("◔", "\u{f09e}")
}
pub fn mixes() -> &'static str {
    pick("◉", "\u{f0e7}")
}
pub fn playlists() -> &'static str {
    pick("≣", "\u{f0cb}")
}
pub fn albums() -> &'static str {
    pick("◎", "\u{f51f}")
}
pub fn tracks() -> &'static str {
    pick("♪", "\u{f886}")
}
pub fn profiles() -> &'static str {
    pick("☺", "\u{f007}")
}
pub fn settings() -> &'static str {
    pick("⚙", "\u{f013}")
}

/// The mark on a favourite track.
pub fn favourite() -> &'static str {
    pick("♥", "\u{f004}")
}

// The transport controls.
//
// The plain set has a fault a nerd font does not: the pause is two half
// blocks, `▌▌`, because no single-character pause glyph is in a terminal
// font -- so play and pause are different widths and the row has to be
// padded to a common one to stop it shifting at every press. A nerd font
// has both as one character, and the padding does nothing.

pub fn play() -> &'static str {
    pick("▶", "\u{f04b}")
}

pub fn pause() -> &'static str {
    // U+01C1, one cell wide and unambiguously so. It replaced `▌▌`, which
    // was two cells: the row had to be padded to a common width, and the
    // button sat off centre between the skip buttons in the plain set. Not
    // U+23F8, the pause emoji -- terminals draw those double width or fall
    // back to a font that draws them a size apart from every other control.
    pick("\u{01c1}", "\u{f04c}")
}

pub fn previous() -> &'static str {
    pick("⏮", "\u{f048}")
}

pub fn next() -> &'static str {
    pick("⏭", "\u{f051}")
}

pub fn shuffle() -> &'static str {
    pick("⇄", "\u{f074}")
}

/// Repeat-all and repeat-one.
///
/// One cell each, in both sets: a control that changes width when it is
/// turned on widens the row and pushes the others sideways. The plain set
/// uses the circled digit rather than a superscript beside the arrow, which
/// would have been a second cell.
pub fn repeat() -> &'static str {
    pick("⟳", "\u{f01e}")
}

pub fn repeat_one() -> &'static str {
    // U+F0E2 rather than the Material Design "repeat-once" at U+F0456: that
    // one is outside the private use area, and a font without it drops back
    // to a fallback that draws it a size apart from the other controls --
    // the same fault the shuffle arrow had.
    pick("\u{2460}", "\u{f0e2}")
}

/// Hold the icon set still for the length of a test.
///
/// The setting is process-wide and the suite runs in parallel, so a test
/// that switches sets would otherwise be read half-way through by another
/// one drawing a bar. Every test that touches it takes this, and puts the
/// old value back on the way out.
#[cfg(test)]
pub struct Fixed {
    was: bool,
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Fixed {
    pub fn at(on: bool) -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        // A poisoned lock means another test panicked while holding it,
        // which says nothing about this one: take it anyway.
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let was = nerd_font();
        set_nerd_font(on);
        Self { was, _guard: guard }
    }

    /// Switch sets without giving up the lock.
    pub fn set(&self, on: bool) {
        set_nerd_font(on);
    }
}

#[cfg(test)]
impl Drop for Fixed {
    fn drop(&mut self) {
        set_nerd_font(self.was);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_set_is_what_an_unconfigured_app_draws() {
        let _fixed = Fixed::at(false);
        // Every plain icon is a glyph a terminal draws without a patched
        // font: an app that opens full of empty boxes looks broken.
        for icon in [
            music(),
            explore(),
            feed(),
            mixes(),
            playlists(),
            albums(),
            tracks(),
            profiles(),
            settings(),
            favourite(),
        ] {
            let c = icon.chars().next().expect("an icon is not empty");
            assert!(
                (c as u32) < 0xE000 || (c as u32) > 0xF8FF,
                "{icon:?} is in the private use area, so it needs a nerd font"
            );
        }
    }

    #[test]
    fn turning_it_on_changes_every_icon() {
        let fixed = Fixed::at(false);
        let plain: Vec<&str> = vec![music(), explore(), tracks(), settings(), favourite()];
        fixed.set(true);
        let nerd: Vec<&str> = vec![music(), explore(), tracks(), settings(), favourite()];

        assert_ne!(plain, nerd, "the setting changed nothing");
        for icon in &nerd {
            let c = icon.chars().next().expect("an icon is not empty");
            assert!(
                (0xE000..=0xF8FF).contains(&(c as u32)),
                "{icon:?} is not a nerd-font glyph"
            );
        }
    }
}
