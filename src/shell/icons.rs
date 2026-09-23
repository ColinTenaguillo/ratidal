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

fn pick(plain: &'static str, nerd: &'static str) -> &'static str {
    if nerd_font() {
        nerd
    } else {
        plain
    }
}

pub fn music() -> &'static str {
    // A house, not a note: this is the home page, and the note belongs to
    // the Tracks section below, where the two were otherwise a list apiece
    // and hard to tell apart at one cell.
    pick("♫", "\u{f015}")
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
    // `md-playlist_music_outline`: a list with a note on it, rather than
    // the numbered list the Font Awesome block offers -- which is a list
    // of anything and read as one beside the Tracks entry.
    pick("≣", "\u{f0cb9}")
}
pub fn albums() -> &'static str {
    // `md-album`: a record. The Font Awesome circle before it was a
    // circle, which said nothing about what the section holds.
    pick("◎", "\u{f0025}")
}
pub fn tracks() -> &'static str {
    // U+F001, not the note at U+F886: that codepoint is not in the Font
    // Awesome block every nerd font carries, and drew an empty box in all
    // four builds it was tried against. A list here read as Playlists'
    // list, so the note moved down from the home page instead.
    pick("♪", "\u{f001}")
}
pub fn profiles() -> &'static str {
    pick("☺", "\u{f007}")
}
pub fn settings() -> &'static str {
    pick("⚙", "\u{f013}")
}

/// The mark on a track whose lyrics are explicit.
///
/// A plain `E` is what every music client uses and what TIDAL draws. The
/// nerd-font set gets `md-alpha_e_box` at U+F0B0C, the same letter in a
/// box -- the badge itself rather than a warning sign standing in for it.
///
/// Verified present in all four nerd fonts installed here: this is the
/// Material Design range, where U+F886 once left an empty box.
pub fn explicit() -> &'static str {
    pick("E", "\u{f0b0c}")
}

/// The mark on the row that is playing.
///
/// The outlined note, `nf-md-music_note_outline` at U+F0F74.
///
/// Distinct from the Tracks section's own filled note and from the
/// player's play triangle, either of which made a row read as one of
/// those instead.
///
/// The outline is not beside the plain note: U+F0387 is `md-music_note`
/// and U+F038A is `md-music_note_off`, a note with a slash through it that
/// drew an ordinary track as struck out. The outlined one sits three
/// thousand codepoints away at U+F0F74, so the glyph name is what to check
/// rather than the neighbourhood.
///
/// Verified present in all four nerd fonts installed here: this range is
/// where U+F886 used to live, and that one is in none of them.
pub fn playing() -> &'static str {
    // A filled triangle in the plain set: `\u{266a}` is the Tracks section's
    // own icon, so the two sat on one screen meaning different things.
    pick("\u{25b8}", "\u{f0f74}")
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

    /// The nine sidebar entries, in the order they are drawn.
    fn sidebar_icons() -> Vec<&'static str> {
        vec![
            music(),
            explore(),
            feed(),
            mixes(),
            playlists(),
            albums(),
            tracks(),
            profiles(),
            settings(),
        ]
    }

    #[test]
    fn nothing_drawn_beside_a_track_looks_like_anything_else_there() {
        // The marks on a row share the line with a title, and the sidebar
        // sits beside them on the same screen. `playing` was given the
        // Tracks note and then the play triangle, and each time it read as
        // the thing it had borrowed from.
        for on in [false, true] {
            let _fixed = Fixed::at(on);
            let mut all = sidebar_icons();
            all.extend([explicit(), playing(), favourite()]);
            for (i, a) in all.iter().enumerate() {
                for b in all.iter().skip(i + 1) {
                    assert_ne!(a, b, "two things on screen draw {a:?} (nerd font: {on})");
                }
            }
        }
    }

    #[test]
    fn no_two_sidebar_entries_draw_the_same_icon() {
        // The column is one cell wide, so the icon is the whole of what
        // tells one entry from another at a glance. Tracks and Playlists
        // were both a list, which read as the same row twice.
        for on in [false, true] {
            let _fixed = Fixed::at(on);
            let icons = sidebar_icons();
            for (i, a) in icons.iter().enumerate() {
                for b in icons.iter().skip(i + 1) {
                    assert_ne!(a, b, "two sidebar entries draw {a:?} (nerd font: {on})");
                }
            }
        }
    }

    #[test]
    fn every_nerd_glyph_is_one_a_font_here_was_checked_for() {
        // U+F886 was a music note that drew an empty box in every font it
        // was tried against: nerd fonts v3 moved that range, and nothing
        // caught it because nothing could. A test cannot open the user's
        // font, so this is the honest version of that guard -- every
        // codepoint drawn is one that was looked up by hand in the four
        // fonts installed on the machine this was written on, and the list
        // below is that record.
        //
        // Adding a glyph means checking it the same way and adding it
        // here. The alternative was a range rule, which said no to
        // `md-album` and `md-alpha_e_box` while saying nothing about
        // whether any particular glyph exists.
        const CHECKED: &[u32] = &[
            0xf015,  // md/fa home
            0xf002,  // search
            0xf09e,  // feed
            0xf0e7,  // mixes
            0xf0cb9, // md-playlist_music_outline
            0xf0025, // md-album
            0xf001,  // fa-music
            0xf007,  // profiles
            0xf013,  // settings
            0xf004,  // favourite
            0xf0b0c, // md-alpha_e_box
            0xf0f74, // md-music_note_outline
        ];
        let _fixed = Fixed::at(true);
        for icon in sidebar_icons()
            .into_iter()
            .chain([explicit(), playing(), favourite()])
        {
            let c = icon.chars().next().expect("an icon is not empty") as u32;
            assert!(
                CHECKED.contains(&c),
                "U+{c:04X} has not been looked up in a real font; check it \
                 and add it to CHECKED"
            );
        }
    }

    #[test]
    fn the_marks_on_a_row_follow_the_set_as_well() {
        // The explicit `E` and the playing note were written into the row
        // renderers rather than taken from here, so they stayed plain while
        // every icon around them changed.
        let fixed = Fixed::at(false);
        assert_eq!(explicit(), "E", "the letter every client uses");
        assert_eq!(playing(), "\u{25b8}", "a triangle, not the Tracks note");
        fixed.set(true);
        for icon in [explicit(), playing()] {
            let c = icon.chars().next().expect("an icon is not empty") as u32;
            assert!(is_nerd_glyph(c), "U+{c:04X} is not a nerd-font glyph");
        }
    }

    #[test]
    fn turning_it_on_changes_every_icon() {
        let fixed = Fixed::at(false);
        let plain: Vec<&str> = vec![
            music(),
            explore(),
            tracks(),
            settings(),
            favourite(),
            explicit(),
            playing(),
        ];
        fixed.set(true);
        let nerd: Vec<&str> = vec![
            music(),
            explore(),
            tracks(),
            settings(),
            favourite(),
            explicit(),
            playing(),
        ];

        assert_ne!(plain, nerd, "the setting changed nothing");
        for icon in &nerd {
            let c = icon.chars().next().expect("an icon is not empty") as u32;
            assert!(is_nerd_glyph(c), "U+{c:04X} is not a nerd-font glyph");
        }
    }

    /// Where nerd fonts actually put their glyphs.
    ///
    /// The Private Use Area holds most of them, but the Material Design
    /// set sits in the supplementary plane at U+F0000 and up -- the
    /// outlined note the playing mark uses is one of those, and a check
    /// that only knew about the PUA called it not a glyph at all.
    fn is_nerd_glyph(c: u32) -> bool {
        (0xE000..=0xF8FF).contains(&c) || (0xF0000..=0xFFFFD).contains(&c)
    }
}
