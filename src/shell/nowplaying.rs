use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme::Palette;
use crate::domain::Track;

#[derive(Debug, Default, Clone)]
pub struct NowPlaying {
    pub track: Option<Track>,
    pub position: Duration,
    pub playing: bool,
    /// The DELIVERED quality, e.g. "24-bit 44.1kHz". Never the requested one.
    pub quality: Option<String>,
    pub tier: Tier,
}

/// The queue's modes, as the bar draws them.
///
/// Passed in rather than copied into `NowPlaying`: the queue is the only
/// thing that knows them, and a copy went stale every time the bar was
/// reset — a failed track cleared the modes while the queue still had them.
#[derive(Debug, Default, Clone, Copy)]
pub struct Modes {
    pub shuffled: bool,
    pub repeat: crate::playback::Repeat,
}

/// How good the stream actually is, which decides the badge's colour.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Hi-res: more than 16 bits, or above CD sample rate.
    Max,
    /// CD-quality lossless.
    High,
    #[default]
    Low,
}

impl Tier {
    /// Classify what the decoder reported.
    ///
    /// Derived from the delivered stream rather than from what was asked
    /// for: requesting hi-res and being handed AAC is exactly the case the
    /// badge exists to make visible.
    ///
    /// TIDAL's own label is the authority, not the bit depth. A LOSSLESS
    /// stream comes back with `bitDepth: null` — checked against a real
    /// library, where every HIGH track reports a null depth and an AAC
    /// codec — so classifying on depth alone put CD-quality lossless in the
    /// same bucket as lossy.
    ///
    /// The boundary between the top two is TIDAL's too: it labels
    /// `24-bit 44.1kHz` MAX and shows it in amber, even though 44.1kHz is
    /// the CD rate and a stricter reading would reserve hi-res for anything
    /// above 48kHz.
    pub fn of_quality(
        delivered: crate::domain::Quality,
        bit_depth: Option<u8>,
        sample_rate: u32,
    ) -> Self {
        use crate::domain::Quality;
        match delivered {
            Quality::HiResLossless => Tier::Max,
            Quality::Lossless => {
                // A lossless container carrying more than CD resolution is
                // hi-res whatever the label said.
                if bit_depth.is_some_and(|b| b > 16) || sample_rate > 48_000 {
                    Tier::Max
                } else {
                    Tier::High
                }
            }
            // HIGH is AAC — the manifest's codec is mp4a.40.2. Lossy, whatever
            // its name suggests.
            Quality::High | Quality::Low => Tier::Low,
        }
    }

    /// Classify from the decoder's numbers alone, for callers with no label.
    ///
    /// Only reaches the right answer when a bit depth is present, which is
    /// why the badge uses `of_quality` instead.
    pub fn of(bit_depth: Option<u8>, sample_rate: u32) -> Self {
        match bit_depth {
            Some(bits) if bits > 16 || sample_rate > 48_000 => Tier::Max,
            Some(_) => Tier::High,
            None => Tier::Low,
        }
    }
}

/// How much of the window's width the progress control takes.
///
/// Measured from the web client: its track runs 584..1216 of 1800, so 35%
/// of the window, and its midpoint sits at exactly 50% — centred on the
/// window, not on whatever space is left between the track and the badges.
const PROGRESS_WIDTH_PCT: u16 = 35;

/// Columns between each time and the bar.
///
/// The web client leaves 16px on both sides — its elapsed time ends at 568
/// and the track starts at 584, and the same gap again at the far end. At
/// roughly 7px a column that is two, and it has to be the same either side
/// or the times read as belonging to whichever they touch.
const TIME_GUTTER: u16 = 2;

/// Find the bar in a rendered frame by the two colours it is drawn in.
///
/// Not by its glyph: the unplayed track is the same rule character as the
/// bar's own top border, so a search by shape returns the border instead —
/// which the geometry tests caught, twice.
#[cfg(test)]
fn find_progress(
    buf: &ratatui::buffer::Buffer,
    palette: &Palette,
) -> Option<super::geometry::Span> {
    super::geometry::longest_run_coloured(buf, &[palette.text, palette.track])
}

/// The play and pause marks.
///
/// Two LEFT half blocks for the pause, not a right and a left: each sits
/// against the left edge of its own cell, so the gap between them is what
/// reads as a pause. `▐▌` puts them either side of the cell join and they
/// meet in the middle as one solid block, which says nothing. It is the pair
/// spotify-player settled on for the same reason.
///
/// A terminal has one glyph size — there is no way to draw the play button
/// larger than its neighbours, as the web client does — so the emphasis is
/// carried by colour and weight instead.
/// U+25B6, not one of the geometric variants that sit better in the cell:
/// those are missing from most terminal fonts and render as a box or as
/// nothing at all. A triangle that is a column off centre beats a triangle
/// nobody can see.
/// The plain glyphs, kept as constants for the tests to name. The bar reads
/// them through `icons`, which swaps in the nerd-font set when that is on.
#[cfg(test)]
pub(super) const PLAY: &str = "▶";

/// The shuffle button.
///
/// U+21C4, not the U+2928 that reads as "shuffle" in a font that has it:
/// most terminal fonts do not, so it came from a fallback and drew a size
/// apart from every other control — visibly smaller when it was lit than
/// when it was not. The same fault the play triangle had.
#[cfg(test)]
pub(super) const SHUFFLE: &str = "⇄";
#[cfg(test)]
pub(super) const PAUSE: &str = "▌▌";

/// The rows a bar actually draws into, once its top and bottom margins are
/// taken off.
///
/// The web client leaves 18px clear above and below a 52px cover in an 88px
/// bar. On a character grid that is one blank row each side, and every block
/// in the bar has to agree on where that leaves them — otherwise the cover
/// sits in the margin while the text sits below it.
fn content_band(area: Rect) -> Rect {
    if area.height <= 2 {
        return area;
    }
    Rect {
        y: area.y + 1,
        height: area.height - 2,
        ..area
    }
}

/// m:ss, with minutes uncapped (a 90-minute mix reads "90:00").
pub fn format_time(d: Duration) -> String {
    let total = d.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

/// Fraction played, clamped to 0.0..=1.0. LineGauge panics outside that range,
/// and a reported position can exceed the container's stated duration.
pub fn progress(position: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        return 0.0;
    }
    (position.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0)
}

pub fn render(frame: &mut Frame, area: Rect, palette: &Palette, state: &NowPlaying, modes: Modes) {
    render_with_cover(frame, area, palette, state, modes, |_, _, _, _| false)
}

/// As [`render`], but able to draw the track's cover as a thumbnail.
///
/// `draw_cover` returns false when it could not draw one, and a coloured
/// block stands in — so the bar keeps its shape on terminals with no image
/// protocol, which is most of them.
pub fn render_with_cover<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &NowPlaying,
    modes: Modes,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let block = Block::default().borders(Borders::TOP);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let Some(track) = &state.track else {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                "Nothing playing",
                palette.subtitle(),
            )),
            inner,
        );
        return;
    };

    // The web client's proportions: a quarter for the track, the transport
    // centred in what is left, and a fixed strip on the right for the badge
    // and its neighbours.
    let right_width = 24.min(inner.width / 3);
    let left_width = (inner.width / 4)
        .max(20)
        .min(inner.width.saturating_sub(right_width));
    let left = Rect {
        width: left_width,
        ..inner
    };
    let right = Rect {
        x: inner.x + inner.width - right_width,
        width: right_width,
        ..inner
    };
    let centre = Rect {
        x: left.x + left.width,
        width: inner
            .width
            .saturating_sub(left.width)
            .saturating_sub(right.width),
        ..inner
    };

    render_track(frame, left, palette, track, &mut draw_cover);
    render_transport(frame, centre, inner, palette, state, modes, track);
    render_badges(frame, right, palette, state);
}

/// Cover, title, artists, album — the web client's left-hand block.
fn render_track<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    track: &Track,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    // The web client's cover is 52px in an 88px bar, with 18px of clear
    // space above and below — it sits in the bar, not across it. On a
    // character grid that is one blank row, three of cover, one blank.
    let thumb_h = content_band(area).height;
    // Square at this terminal's own cell, the same rule the cards follow.
    // A fixed doubling assumes a cell exactly twice as tall as it is wide
    // and leaves the cover squat or stretched anywhere else.
    let thumb_w = super::carousel::square_width(thumb_h).min(area.width / 3);
    // And the same air to its left, so it is inset rather than flush.
    let thumb_x = area.x + 1;
    if thumb_w > 0 && thumb_x + thumb_w <= area.x + area.width {
        let thumb = Rect {
            x: thumb_x,
            y: content_band(area).y,
            width: thumb_w,
            height: thumb_h,
        };
        let drew = match &track.cover {
            Some(url) => draw_cover(frame, thumb, url, super::artwork::Shape::Square),
            None => false,
        };
        if !drew {
            frame.render_widget(
                Block::default().style(Style::default().bg(palette.placeholder)),
                thumb,
            );
        }
    }

    // 16px between cover and text on the web, against a 52px cover: about a
    // third of its width, so two columns here.
    let text_x = thumb_x + thumb_w + 2;
    if text_x >= area.x + area.width {
        return;
    }
    // A gutter, so a long artist name does not run into the transport.
    let text_w = (area.x + area.width - text_x).saturating_sub(2);

    // Title, artists, album — three lines, as the web client stacks them.
    let lines: [(&str, ratatui::style::Style); 3] = [
        (track.title.as_str(), palette.title()),
        (track.artist.as_str(), palette.subtitle()),
        (track.album.as_str(), palette.subtitle()),
    ];
    // The same band as everything else, so the three blocks line up.
    let text_y = content_band(area).y;
    for (i, (text, style)) in lines.iter().enumerate() {
        let y = text_y + i as u16;
        if text.is_empty() || y >= area.y + area.height {
            continue;
        }
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                super::carousel::truncate(text, text_w),
                *style,
            )),
            Rect {
                x: text_x,
                y,
                width: text_w,
                height: 1,
            },
        );
    }
}

/// The transport controls, and the progress bar with a time either side.
/// `full` is the whole bar, which is what the transport is centred on.
/// Centring inside `area` instead put it off to one side, because the blocks
/// either side of it are not the same width.
fn render_transport(
    frame: &mut Frame,
    area: Rect,
    full: Rect,
    palette: &Palette,
    state: &NowPlaying,
    modes: Modes,
    track: &Track,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // The bar keeps a blank row top and bottom, so its content sits in the
    // band between them — anchoring to `area.y` would put this in the margin.
    let band = content_band(area);

    // Shuffle, previous, play/pause, next, repeat — the same five the web
    // client shows, in the same order. Shuffle and repeat light up when they
    // are on, which is the only thing that says a mode is in effect.
    // Play is the one control the eye should land on, so it is the brightest
    // and heaviest of the five, as the web client draws it much larger. A
    // terminal has one glyph size, so weight and colour carry that instead.
    // The pause pair is two half blocks rather than U+23F8, which most fonts
    // draw thin enough to disappear next to the triangle.
    // Two LEFT half blocks, not a right and a left: each sits against the
    // left edge of its own cell, so the gap between them is what reads as a
    // pause. `▐▌` put them either side of the join and they met in the
    // middle as one solid block, which says nothing.
    //
    // Padded to a common width so the row does not shift sideways at every
    // press — the widths come from the glyphs rather than being counted by
    // hand, so changing one cannot break the alignment.
    let cells = super::icons::play()
        .chars()
        .count()
        .max(super::icons::pause().chars().count());
    // No lead-in, and the padding does nothing: play and pause are one cell
    // each in both sets. The column that used to be here corrected a
    // two-cell pause, `▌▌`, whose ink hugged the left edge of each cell --
    // with a one-cell pause it put the button off centre instead.
    let play = format!(
        "{:width$}",
        if state.playing {
            super::icons::play()
        } else {
            super::icons::pause()
        },
        width = cells
    );
    let on = palette.accent_text();
    let off = palette.subtitle();
    use crate::playback::Repeat;
    use ratatui::text::Span;
    let transport = ratatui::text::Line::from(vec![
        Span::styled(
            super::icons::shuffle(),
            if modes.shuffled { on } else { off },
        ),
        Span::styled(
            format!("   {}   ", super::icons::previous()),
            palette.subtitle(),
        ),
        Span::styled(play, palette.play_button()),
        Span::styled(
            format!("   {}   ", super::icons::next()),
            palette.subtitle(),
        ),
        // Repeat-one is marked apart from repeat-all: the same glyph in the
        // same colour for two different modes says nothing. The superscript
        // is a second cell, so the other modes are padded to match — left
        // to differ, turning repeat on widened the row and pushed the
        // controls out of the pane.
        Span::styled(
            match modes.repeat {
                Repeat::One => super::icons::repeat_one(),
                _ => super::icons::repeat(),
            },
            if modes.repeat == Repeat::Off { off } else { on },
        ),
    ]);
    frame.render_widget(
        Paragraph::new(transport).alignment(ratatui::layout::Alignment::Center),
        Rect {
            y: band.y,
            height: 1,
            ..full
        },
    );

    // The times sit either side of the bar rather than inside it, which is
    // what the web client does and what leaves the bar unbroken.
    //
    // A row below the transport controls rather than immediately under
    // them: the bar sat tight against the buttons with two clear rows below
    // it, so the whole group read as pushed up against the top of the bar.
    let y = band.y + 2;
    if y >= band.y + band.height {
        return;
    }
    let elapsed = format_time(state.position);
    let total = format_time(track.duration);
    let time_w = elapsed.chars().count().max(total.chars().count()) as u16;

    // The web client's progress bar is about a third of the window, not the
    // whole space between the track and the badges. Left to fill its column
    // it stretched right across the screen and read as a rule, not a
    // control.
    let bar_w = (PROGRESS_WIDTH_PCT.min(100) as u32 * full.width as u32 / 100) as u16;
    // The times hang outside the bar, so the whole group has to fit.
    let bar_w = bar_w.min(full.width.saturating_sub((time_w + TIME_GUTTER) * 2));
    // Round to an even width. Centring an odd run in an even field leaves
    // the midpoint half a cell off, which is visible as a lean once the bar
    // is this long; an even one lands on centre exactly.
    let bar_w = if full.width % 2 == bar_w % 2 {
        bar_w
    } else {
        bar_w.saturating_sub(1)
    };
    if bar_w == 0 {
        return;
    }

    // The BAR's midpoint lands at 50%, as the web client's does — not the
    // group's. Centring the group instead leaves the bar off by half the
    // difference, since the times and gutters are not symmetric about it
    // once integer cells round.
    let bar_x = full.x + (full.width.saturating_sub(bar_w)) / 2;
    let elapsed_x = bar_x.saturating_sub(time_w + TIME_GUTTER);

    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(elapsed, palette.subtitle()))
            .alignment(ratatui::layout::Alignment::Right),
        Rect {
            x: elapsed_x,
            y,
            width: time_w,
            height: 1,
        },
    );
    render_progress(
        frame,
        Rect {
            x: bar_x,
            y,
            width: bar_w,
            height: 1,
        },
        palette,
        progress(state.position, track.duration),
    );
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(total, palette.subtitle())),
        Rect {
            x: bar_x + bar_w + TIME_GUTTER,
            y,
            width: time_w,
            height: 1,
        },
    );
}

/// The partial blocks, in eighths of a cell.
///
/// A bar 33 columns wide advances a whole cell at a time, which is three
/// percent of the track in one jump — the head sits still, then leaps. These
/// fill the last cell by eighths, so it moves at every percent instead.
const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

fn render_progress(frame: &mut Frame, area: Rect, palette: &Palette, ratio: f64) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    // In eighths of a cell rather than whole ones.
    let eighths = (ratio.clamp(0.0, 1.0) * area.width as f64 * 8.0).round() as u32;
    let full = (eighths / 8).min(area.width as u32) as u16;
    let part = (eighths % 8) as usize;

    // The track is drawn in the same full block as the played part, told
    // apart by colour alone. A thinner glyph left a step down at the join —
    // and a gap where a partial eighth met it, since the eighths are full
    // height and the rule was not.
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(
            "█".repeat(area.width as usize),
            Style::default().fg(palette.track),
        )),
        area,
    );

    // The played part, in the brighter colour. Its partial cell is drawn
    // over the track's own colour rather than over nothing, so the eighth
    // that is not yet played still reads as track instead of a notch.
    let full = full.min(area.width);
    if full > 0 {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                "█".repeat(full as usize),
                Style::default().fg(palette.text),
            )),
            Rect {
                width: full,
                ..area
            },
        );
    }
    if part > 0 && full < area.width {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                EIGHTHS[part],
                Style::default().fg(palette.text).bg(palette.track),
            )),
            Rect {
                x: area.x + full,
                width: 1,
                ..area
            },
        );
    }
}

/// The queue and volume marks, and the delivered-quality badge.
fn render_badges(frame: &mut Frame, area: Rect, palette: &Palette, state: &NowPlaying) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Level with the artist rather than the title: the badge is the only
    // thing on the right, so putting it on the middle of the three lines
    // balances the bar instead of weighting it at the top.
    let band = content_band(area);
    let y = band.y + (band.height.min(3) / 2);
    let Some(quality) = &state.quality else {
        return;
    };

    // No surrounding spaces: they were the padding a filled badge needed,
    // and without a background they read as the badge sitting off the edge.
    let badge = quality.clone();
    let badge_w = (badge.chars().count() as u16).min(area.width);
    let badge_x = area.x + area.width - badge_w;

    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(
            badge,
            palette.quality_badge(state.tier),
        )),
        Rect {
            x: badge_x,
            y,
            width: badge_w,
            height: 1,
        },
    );
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_transport_uses_glyphs_a_terminal_font_has() {
        // Twice now a control has been drawn with a codepoint most fonts
        // lack: U+2BC8 for play, which showed as nothing, and U+2928 for
        // shuffle, which came from a fallback and drew a size apart from
        // its neighbours — visibly smaller lit than unlit.
        //
        // These blocks are the ones a monospace font is expected to carry.
        // Anything past them is a fallback waiting to happen.
        for (what, glyph) in [("play", PLAY), ("pause", PAUSE), ("shuffle", SHUFFLE)] {
            for c in glyph.chars() {
                let cp = c as u32;
                let known = (0x2190..=0x21FF).contains(&cp)  // arrows
                    || (0x2580..=0x259F).contains(&cp)       // block elements
                    || (0x25A0..=0x25FF).contains(&cp)       // geometric shapes
                    || (0x2600..=0x26FF).contains(&cp);      // misc symbols
                assert!(
                    known,
                    "{what} is U+{cp:04X}, outside the blocks a terminal font carries"
                );
            }
        }
    }
    use std::time::Duration;

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    /// The bar alone, so a test can read the glyphs it drew.
    fn progress_row(width: u16, ratio: f64) -> String {
        let buf = super::super::geometry::draw(width, 1, |f, area, palette| {
            render_progress(f, area, palette, ratio);
        });
        super::super::geometry::row(&buf, 0)
    }

    /// How many cells of the bar are played, read off the colour: both
    /// halves are drawn in the same glyph, so the shape cannot tell them
    /// apart.
    fn played_cells(width: u16, ratio: f64) -> usize {
        let palette = Palette::detect();
        let buf = super::super::geometry::draw(width, 1, |f, area, palette| {
            render_progress(f, area, palette, ratio);
        });
        (0..width)
            .filter(|x| buf[(*x, 0)].fg == palette.text)
            .count()
    }

    #[test]
    fn the_bar_advances_by_eighths_of_a_cell() {
        // A whole-cell bar 32 wide moves in three-percent jumps: the head
        // sits still, then leaps. Three ratios inside one cell have to draw
        // three different things.
        let a = progress_row(32, 0.330);
        let b = progress_row(32, 0.340);
        let c = progress_row(32, 0.350);
        assert_ne!(a, b, "a percent apart must not draw the same bar\n{a}\n{b}");
        assert_ne!(b, c, "{b}\n{c}");
    }

    #[test]
    fn a_partial_cell_still_reads_as_track_behind_the_head() {
        // The eighths are drawn over nothing, so the part of that cell not
        // yet played showed the pane behind the bar — a notch in what
        // should be one unbroken shape.
        let palette = Palette::detect();
        let buf = super::super::geometry::draw(30, 1, |f, area, palette| {
            // A ratio that lands mid-cell.
            render_progress(f, area, palette, 0.13);
        });

        let head = (0..30u16)
            .find(|x| {
                let sym = buf[(*x, 0)].symbol();
                sym != "█" && sym != " "
            })
            .expect("a partial cell");
        assert_eq!(
            buf[(head, 0)].bg,
            palette.track,
            "the unplayed part of the head's own cell is still track: {:?}",
            super::super::geometry::row(&buf, 0)
        );
    }

    #[test]
    fn the_players_cover_is_square_at_this_terminals_cell() {
        // It doubled the height, which is square only on a cell exactly
        // twice as tall as it is wide — the same assumption the cards were
        // built on and had to be taken back out of.
        // At the default card width the proportion happens to be two, so
        // comparing against it proves nothing: the cover is drawn and
        // measured against the card's own cover instead.
        use crate::shell::{carousel, geometry};
        let mut t = track();
        // Without one the cover is never asked for, so nothing is measured.
        t.cover = Some("https://example.invalid/c.jpg".into());
        let state = NowPlaying {
            track: Some(t),
            position: Duration::from_secs(1),
            playing: true,
            quality: None,
            tier: Tier::Low,
        };
        let mut drawn = Vec::new();
        let _ = geometry::draw(100, crate::shell::layout::NOW_PLAYING_HEIGHT, |f, a, p| {
            render_with_cover(f, a, p, &state, Modes::default(), |_, r, _, _| {
                drawn.push(r);
                true
            })
        });
        let cover = drawn.first().copied().expect("the cover was drawn");
        assert_eq!(
            cover.width,
            carousel::square_width(cover.height),
            "the cover is square by the same rule a card's is, rather than \
             by a doubling that only holds on a 1:2 cell"
        );
    }

    #[test]
    fn the_quality_badge_sits_level_with_the_artist() {
        // It is the only thing on the right, so on the top line it weighted
        // the bar's corner; on the middle of the three it balances.
        use crate::shell::geometry;
        let buf = bar(90);
        let badge = geometry::find(&buf, "24-bit").expect("the badge");
        let artist = geometry::find(&buf, "Kendrick").expect("the artist");
        assert_eq!(
            badge.row,
            artist.row,
            "the badge shares the artist's line\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn the_bar_is_one_unbroken_shape() {
        // Both halves are the same full block, told apart by colour. A
        // thinner glyph for the track left a step down at the join, and a
        // gap where a partial eighth met it.
        let palette = Palette::detect();
        let buf = super::super::geometry::draw(32, 1, |f, area, palette| {
            render_progress(f, area, palette, 0.5);
        });

        for x in 0..32u16 {
            assert_eq!(
                buf[(x, 0)].symbol(),
                "█",
                "cell {x} breaks the bar's shape: {:?}",
                super::super::geometry::row(&buf, 0)
            );
        }
        // And the colour still says where the head is.
        assert_eq!(buf[(0, 0)].fg, palette.text, "played at the left");
        assert_eq!(buf[(31, 0)].fg, palette.track, "track at the right");
    }

    #[test]
    fn the_ends_of_the_bar_are_exact() {
        // Nothing played draws no block at all, and a finished track fills
        // every cell — a partial block at either end reads as a rounding
        // fault rather than a position.
        assert_eq!(played_cells(20, 0.0), 0, "nothing played");
        assert_eq!(played_cells(20, 1.0), 20, "a finished track fills the bar");
    }

    #[test]
    fn a_partial_block_never_runs_past_the_end() {
        // Rounding up in the last cell would push an eighth past the bar's
        // width and shunt whatever follows it.
        for pct in 90..=100 {
            let row = progress_row(20, f64::from(pct) / 100.0);
            assert_eq!(
                row.chars().count(),
                20,
                "at {pct}% the bar is still 20 cells:\n{row}"
            );
        }
    }

    fn track() -> crate::domain::Track {
        crate::domain::Track {
            tags: vec!["LOSSLESS".into(), "HIRES_LOSSLESS".into()],
            ..crate::domain::Track::sample(
                "Money Trees",
                "Kendrick Lamar, Jay Rock",
                Duration::from_secs(387),
            )
        }
    }

    #[test]
    fn play_and_pause_start_on_the_same_column() {
        // The row must not shift sideways at a press. Both marks are one
        // cell now, so they share a column outright rather than by way of
        // the padding that a two-cell pause once needed.
        use crate::shell::geometry;
        // Which set is in use is a global, so a nerd-font test running
        // beside this one can switch it between the draw and the lookup
        // and leave us searching the buffer for the other set's glyph.
        let _fixed = crate::shell::icons::Fixed::at(false);
        let draw = |playing: bool| {
            let state = NowPlaying {
                track: Some(track()),
                position: Duration::from_secs(108),
                playing,
                quality: Some("24-bit".into()),
                tier: Tier::Max,
            };
            geometry::draw(
                76,
                crate::shell::layout::NOW_PLAYING_HEIGHT,
                move |f, a, p| render(f, a, p, &state, Modes::default()),
            )
        };
        let playing = draw(true);
        let paused = draw(false);
        let a = geometry::find(&playing, crate::shell::icons::play())
            .expect("the play mark");
        let b = geometry::find(&paused, crate::shell::icons::pause())
            .expect("the pause mark");
        assert_eq!(
            a.start,
            b.start,
            "both start on the same column\n{}\n{}",
            geometry::text(&playing),
            geometry::text(&paused)
        );
    }

    #[test]
    fn the_quality_badge_is_plain_text_at_the_edge() {
        // It was a filled chip with two decorative marks beside it. The
        // chip drew the eye harder than the track name, and the marks stood
        // for nothing — no key reaches them and no state changes them.
        use crate::shell::geometry;
        let buf = bar(76);
        let badge = geometry::find(&buf, "24-bit").expect("the badge");

        assert_eq!(
            buf[(badge.start, badge.row)].bg,
            ratatui::style::Color::Reset,
            "no background behind it\n{}",
            geometry::text(&buf)
        );

        let text = geometry::text(&buf);
        assert!(!text.contains('≣'), "the marks are gone:\n{text}");
        assert!(!text.contains('◍'), "both of them:\n{text}");
    }

    #[test]
    fn the_row_is_the_same_width_in_every_repeat_mode() {
        // The repeat-one mark is a second cell wide, so turning repeat on
        // grew the row and pushed the controls either side of it out of
        // place — or off the pane entirely on a narrow terminal.
        use crate::playback::Repeat;
        use crate::shell::geometry;
        // The row is searched for `SHUFFLE`, the plain glyph, so the set
        // has to stay the plain one while the buffers are read.
        let _fixed = crate::shell::icons::Fixed::at(false);

        let draw = |repeat: Repeat| {
            let state = NowPlaying {
                track: Some(track()),
                position: Duration::from_secs(108),
                playing: true,
                quality: Some("24-bit".into()),
                tier: Tier::Max,
            };
            geometry::draw(
                80,
                crate::shell::layout::NOW_PLAYING_HEIGHT,
                move |f, a, p| {
                    render(
                        f,
                        a,
                        p,
                        &state,
                        Modes {
                            shuffled: false,
                            repeat,
                        },
                    )
                },
            )
        };

        let off = draw(Repeat::Off);
        let shuffle = geometry::find(&off, SHUFFLE).expect("shuffle").start;
        for repeat in [Repeat::All, Repeat::One] {
            let buf = draw(repeat);
            let at = geometry::find(&buf, SHUFFLE).expect("shuffle").start;
            assert_eq!(
                at,
                shuffle,
                "{repeat:?} moved the row\n{}",
                geometry::text(&buf)
            );
        }
    }

    #[test]
    fn the_transport_row_does_not_shift_when_paused() {
        // The play glyph is one cell and a pause pair is two, so the row
        // jumped sideways every time it was pressed.
        use crate::shell::geometry;
        let draw = |playing: bool| {
            let state = NowPlaying {
                track: Some(track()),
                position: Duration::from_secs(108),
                playing,
                quality: Some("24-bit".into()),
                tier: Tier::Max,
            };
            geometry::draw(
                80,
                crate::shell::layout::NOW_PLAYING_HEIGHT,
                move |f, a, p| render(f, a, p, &state, Modes::default()),
            )
        };

        let playing = draw(true);
        let paused = draw(false);
        // The button AFTER the mark: shuffle sits before it and never moves,
        // so comparing that passed however wide the mark was drawn.
        let a = geometry::find(&playing, "⏭").expect("next while playing");
        let b = geometry::find(&paused, "⏭").expect("next while paused");
        assert_eq!(
            a.start,
            b.start,
            "what follows the mark sits in the same columns either way\n{}\n{}",
            geometry::text(&playing),
            geometry::text(&paused)
        );
    }

    #[test]
    fn the_pause_mark_is_one_cell_wide() {
        // It used to be `▌▌`, two left half blocks: no terminal font has a
        // single-character pause, and `▐▌` closes up into a solid block that
        // reads as nothing. Two cells meant the row was padded to a common
        // width and nudged a column right to correct the ink -- and both of
        // those put the button off centre between the skips.
        //
        // U+01C1 is one cell and unambiguously so, which is what lets the
        // padding and the lead-in go.
        //
        // Pinned to the plain set: this is a claim about `ǁ`, and the
        // nerd-font pause is a different glyph that the global could
        // otherwise switch to underneath the assertions.
        use crate::shell::geometry;
        let _fixed = crate::shell::icons::Fixed::at(false);
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: false,
            quality: Some("24-bit".into()),
            tier: Tier::Max,
        };
        let buf = geometry::draw(
            80,
            crate::shell::layout::NOW_PLAYING_HEIGHT,
            move |f, a, p| render(f, a, p, &state, Modes::default()),
        );
        let text = geometry::text(&buf);
        assert!(
            text.contains(crate::shell::icons::pause()),
            "the pause mark is drawn:\n{text}"
        );
        assert_eq!(
            crate::shell::icons::pause().chars().count(),
            1,
            "and it is one cell, or the row needs padding again"
        );
        assert!(
            !text.contains("▐▌"),
            "and never the pair that closes up into a block:\n{text}"
        );
    }

    #[test]
    fn play_is_the_brightest_control_in_the_row() {
        // The web client draws it half again the size of its neighbours; a
        // terminal has one glyph size, so weight and colour carry it.
        use crate::shell::geometry;
        // `PLAY` is the plain glyph spelled out, and the set is a global a
        // neighbouring test can move.
        let _fixed = crate::shell::icons::Fixed::at(false);
        let palette = Palette::detect();
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: true,
            quality: Some("24-bit".into()),
            tier: Tier::Max,
        };
        let buf = geometry::draw(
            80,
            crate::shell::layout::NOW_PLAYING_HEIGHT,
            move |f, a, p| render(f, a, p, &state, Modes::default()),
        );

        let play = geometry::find(&buf, PLAY).expect("the play button");
        let skip = geometry::find(&buf, "⏭").expect("the next button");
        assert_eq!(
            buf[(play.start, play.row)].fg,
            palette.text,
            "play is the white one"
        );
        assert_eq!(
            buf[(skip.start, skip.row)].fg,
            palette.dim,
            "and the skips are dimmer than it"
        );
    }

    #[test]
    fn shuffle_and_repeat_show_whether_they_are_on() {
        // The buttons were drawn in one style whatever the state, so a mode
        // being in effect was invisible.
        use crate::playback::Repeat;
        use crate::shell::geometry;
        // `SHUFFLE` is the plain glyph spelled out, so the set has to be
        // held to the plain one for the width of the test.
        let _fixed = crate::shell::icons::Fixed::at(false);
        let palette = Palette::detect();

        let draw = |shuffled: bool, repeat: Repeat| {
            let state = NowPlaying {
                track: Some(track()),
                position: Duration::from_secs(108),
                playing: true,
                quality: None,
                tier: Tier::Max,
            };
            geometry::draw(
                100,
                crate::shell::layout::NOW_PLAYING_HEIGHT,
                move |f, a, p| render(f, a, p, &state, Modes { shuffled, repeat }),
            )
        };

        let off = draw(false, Repeat::Off);
        let shuffle_off = geometry::find(&off, SHUFFLE).expect("the shuffle button");
        assert_eq!(
            off[(shuffle_off.start, shuffle_off.row)].fg,
            palette.dim,
            "shuffle is dim when it is off"
        );

        let on = draw(true, Repeat::Off);
        let shuffle_on = geometry::find(&on, SHUFFLE).expect("the shuffle button");
        assert_eq!(
            on[(shuffle_on.start, shuffle_on.row)].fg,
            palette.accent,
            "and lit when it is on"
        );

        // Repeat-one has to be told apart from repeat-all, which the same
        // glyph in the same colour would not do.
        let all = geometry::text(&draw(false, Repeat::All));
        let one = geometry::text(&draw(false, Repeat::One));
        assert_ne!(all, one, "the two repeat modes look different");
        assert!(
            one.contains(crate::shell::icons::repeat_one()),
            "repeat-one is marked with its own glyph:\n{one}"
        );
    }

    #[test]
    fn the_bar_sits_a_row_below_the_transport_controls() {
        // Not tight under them: with two clear rows beneath, the whole group
        // read as pushed against the top of the bar.
        use crate::shell::geometry;
        // `PLAY` is the plain glyph spelled out, and the set is a global a
        // neighbouring test can move.
        let _fixed = crate::shell::icons::Fixed::at(false);
        let buf = bar(100);
        let palette = Palette::detect();

        let play = geometry::find(&buf, PLAY).expect("the transport controls");
        let progress = find_progress(&buf, &palette).expect("the bar");
        assert_eq!(
            progress.row,
            play.row + 2,
            "a clear row between the controls and the bar\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn the_bar_stays_inside_the_players_own_rows() {
        // One row lower is one row closer to running out of them; a bar
        // drawn past the band would land on the border or vanish.
        use crate::shell::geometry;
        let palette = Palette::detect();
        for height in 1..=crate::shell::layout::NOW_PLAYING_HEIGHT {
            let state = NowPlaying {
                track: Some(track()),
                position: Duration::from_secs(108),
                playing: true,
                quality: Some("24-bit 44.1kHz".into()),
                tier: Tier::Max,
            };
            let buf = geometry::draw(100, height, move |f, area, p| {
                render(f, area, p, &state, Modes::default())
            });
            if let Some(progress) = find_progress(&buf, &palette) {
                assert!(
                    progress.row < height,
                    "at height {height} the bar is drawn on row {}",
                    progress.row
                );
            }
        }
    }

    /// The bar, drawn at its real height, for the geometry assertions.
    fn bar(width: u16) -> ratatui::buffer::Buffer {
        use crate::shell::geometry;
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: true,
            quality: Some("24-bit 44.1kHz".into()),
            tier: Tier::Max,
        };
        geometry::draw(
            width,
            crate::shell::layout::NOW_PLAYING_HEIGHT,
            move |f, area, p| render(f, area, p, &state, Modes::default()),
        )
    }

    #[test]
    fn the_progress_bar_is_centred_on_the_window() {
        // It was centred inside the middle column instead, and the blocks
        // either side are not the same width — so it sat off to one side.
        // Asserting on content could not see that; only a position can.
        use crate::shell::geometry;
        for width in [80u16, 100, 120, 140, 200] {
            let buf = bar(width);
            let run = find_progress(&buf, &Palette::detect())
                .unwrap_or_else(|| panic!("no progress bar at width {width}"));
            geometry::assert_centred(run, &buf);
        }
    }

    #[test]
    fn the_progress_bar_keeps_the_web_clients_share_of_the_width() {
        // 35% of the window, measured from the running web client. Allowed a
        // point of slack for the cell the parity rounding costs.
        use crate::shell::geometry;
        for width in [100u16, 140, 200] {
            let buf = bar(width);
            let run = find_progress(&buf, &Palette::detect()).expect("a progress bar");
            let pct = run.width_pct(width);
            assert!(
                (33.0..=36.0).contains(&pct),
                "at width {width} the bar takes {pct:.1}%, expected about 35%\n{}",
                geometry::text(&buf)
            );
        }
    }

    #[test]
    fn the_times_sit_the_same_distance_from_the_bar_on_both_sides() {
        // The elapsed time was flush against the bar while the total had a
        // column of air, so the pair read as belonging to different things.
        use crate::shell::geometry;
        let buf = bar(140);
        let run = find_progress(&buf, &Palette::detect()).expect("a progress bar");
        let elapsed = geometry::find(&buf, "1:48").expect("elapsed time");
        let total = geometry::find(&buf, "6:27").expect("total time");

        assert_eq!(
            elapsed.row, run.row,
            "the elapsed time shares the bar's row"
        );
        assert_eq!(total.row, run.row, "and so does the total");

        geometry::assert_gap(elapsed, run, TIME_GUTTER, &buf);
        geometry::assert_gap(run, total, TIME_GUTTER, &buf);
    }

    #[test]
    fn the_cover_has_a_blank_row_above_and_below_it() {
        // The web client leaves 18px clear either side of a 52px cover in an
        // 88px bar. Pressed flush the cover read as pasted into the corner.
        use crate::shell::geometry;
        let buf = bar(140);
        let inner_top = 1; // the bar's own top border
        let last = buf.area.height - 1;

        assert!(
            geometry::occupied(&buf, inner_top).is_none(),
            "the row under the border is the cover's top margin\n{}",
            geometry::text(&buf)
        );
        assert!(
            geometry::occupied(&buf, last).is_none(),
            "and the last row is its bottom margin\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn nothing_is_drawn_hard_against_the_left_edge() {
        // The cover was flush with the frame; the web client insets it.
        use crate::shell::geometry;
        let buf = bar(140);
        for y in 0..buf.area.height {
            if let Some(span) = geometry::occupied(&buf, y) {
                // The top border is a rule and does span the frame.
                if y == 0 {
                    continue;
                }
                assert!(
                    span.start > 0,
                    "row {y} starts at column 0, with no inset\n{}",
                    geometry::text(&buf)
                );
            }
        }
    }

    fn rendered(state: &NowPlaying, width: u16) -> String {
        // The bar's own height: rendering into less clips the progress row,
        // which is exactly what these tests are looking for.
        let h = crate::shell::layout::NOW_PLAYING_HEIGHT;
        let mut terminal = Terminal::new(TestBackend::new(width, h)).unwrap();
        let palette = Palette::detect();
        terminal
            .draw(|frame| render(frame, frame.area(), &palette, state, Modes::default()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_lossless_stream_is_not_demoted_for_reporting_no_bit_depth() {
        // The bug this is here for: TIDAL returns `bitDepth: null` for
        // anything that is not hi-res, so classifying on depth alone put
        // CD-quality lossless in the same bucket as AAC — white, not green.
        use crate::domain::Quality;
        assert_eq!(
            Tier::of_quality(Quality::Lossless, None, 44_100),
            Tier::High,
            "lossless with no reported depth is still lossless"
        );
        // And the old depth-only rule is exactly what got it wrong.
        assert_eq!(Tier::of(None, 44_100), Tier::Low);
    }

    #[test]
    fn high_is_lossy_whatever_its_name_suggests() {
        // TIDAL's HIGH is AAC: the manifest's codec is mp4a.40.2, checked
        // against a real stream. The name is the trap.
        use crate::domain::Quality;
        assert_eq!(Tier::of_quality(Quality::High, None, 44_100), Tier::Low);
        assert_eq!(Tier::of_quality(Quality::Low, None, 44_100), Tier::Low);
    }

    #[test]
    fn a_lossless_container_above_cd_resolution_is_hi_res() {
        // Whatever the label said: 24 bits or a rate above 48kHz is hi-res.
        use crate::domain::Quality;
        assert_eq!(
            Tier::of_quality(Quality::Lossless, Some(24), 44_100),
            Tier::Max
        );
        assert_eq!(
            Tier::of_quality(Quality::Lossless, Some(16), 96_000),
            Tier::Max
        );
        assert_eq!(
            Tier::of_quality(Quality::HiResLossless, Some(24), 44_100),
            Tier::Max
        );
    }

    #[test]
    fn the_tiers_match_what_tidal_shows_for_the_same_stream() {
        // Checked against the web client with a track playing: it labels
        // 24-bit 44.1kHz MAX in amber, not HIGH. Bit depth alone earns the
        // top badge; the sample rate does not have to exceed CD.
        assert_eq!(
            Tier::of(Some(24), 44_100),
            Tier::Max,
            "TIDAL calls this MAX"
        );
        assert_eq!(Tier::of(Some(24), 176_400), Tier::Max);
        // And a CD-rate 16-bit stream is the tier below.
        assert_eq!(Tier::of(Some(16), 44_100), Tier::High);
    }

    #[test]
    fn the_tier_follows_what_was_delivered_not_what_was_asked_for() {
        // The badge exists to make one case visible: asking for hi-res and
        // being handed something less. Classifying the request would defeat
        // the whole point of it.
        assert_eq!(Tier::of(Some(24), 176_400), Tier::Max, "24-bit hi-res");
        assert_eq!(
            Tier::of(Some(24), 44_100),
            Tier::Max,
            "24-bit at CD rate is still hi-res"
        );
        assert_eq!(
            Tier::of(Some(16), 96_000),
            Tier::Max,
            "above CD rate is hi-res"
        );

        assert_eq!(
            Tier::of(Some(16), 44_100),
            Tier::High,
            "CD-quality lossless"
        );
        assert_eq!(Tier::of(Some(16), 48_000), Tier::High);

        // AAC reports no bit depth at all.
        assert_eq!(Tier::of(None, 44_100), Tier::Low, "lossy is not lossless");
    }

    #[test]
    fn each_tier_gets_its_own_colour() {
        // Three tiers that all rendered the same colour would say nothing.
        let p = Palette::detect();
        let max = p.quality_badge(Tier::Max).fg;
        let high = p.quality_badge(Tier::High).fg;
        let low = p.quality_badge(Tier::Low).fg;
        assert_ne!(max, high, "hi-res and lossless must differ");
        assert_ne!(high, low, "lossless and lossy must differ");
        assert_ne!(max, low);
    }

    #[test]
    fn shows_title_artist_and_both_times() {
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: true,
            quality: Some("24-bit 44.1kHz".into()),
            tier: Tier::Max,
        };
        let text = rendered(&state, 100);
        assert!(text.contains("Money Trees"), "title:\n{text}");
        // Truncated to the column, so check the part that survives: the
        // point is that the artist line is drawn at all.
        assert!(text.contains("Kendrick"), "artist:\n{text}");
        assert!(text.contains("1:48"), "elapsed:\n{text}");
        assert!(text.contains("6:27"), "total:\n{text}");
        assert!(text.contains("24-bit"), "quality badge:\n{text}");
    }

    #[test]
    fn formats_times_as_minutes_and_seconds() {
        assert_eq!(format_time(Duration::from_secs(0)), "0:00");
        assert_eq!(format_time(Duration::from_secs(9)), "0:09");
        assert_eq!(format_time(Duration::from_secs(108)), "1:48");
        assert_eq!(format_time(Duration::from_secs(387)), "6:27");
        assert_eq!(format_time(Duration::from_secs(3600)), "60:00");
    }

    #[test]
    fn progress_ratio_is_clamped_to_one() {
        // A position past the reported duration must not panic LineGauge,
        // which requires 0.0..=1.0.
        assert_eq!(
            progress(Duration::from_secs(10), Duration::from_secs(5)),
            1.0
        );
        assert_eq!(progress(Duration::ZERO, Duration::from_secs(10)), 0.0);
        assert!((progress(Duration::from_secs(5), Duration::from_secs(10)) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_zero_duration_track_does_not_divide_by_zero() {
        assert_eq!(progress(Duration::from_secs(3), Duration::ZERO), 0.0);
    }

    #[test]
    fn the_thumbnail_does_not_crowd_out_the_title() {
        // The cover thumb eats into the left column, so the title and artist
        // have to survive the squeeze.
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: true,
            quality: Some("24-bit 44.1kHz".into()),
            tier: Tier::Max,
        };
        let text = rendered(&state, 100);
        assert!(
            text.contains("Money Trees"),
            "title lost to the thumb:\n{text}"
        );
        assert!(
            text.contains("Kendrick"),
            "artist lost to the thumb:\n{text}"
        );
        assert!(text.contains("1:48"), "elapsed lost:\n{text}");
        assert!(text.contains("24-bit"), "quality badge lost:\n{text}");
    }

    #[test]
    fn renders_with_no_track_without_panicking() {
        let text = rendered(&NowPlaying::default(), 60);
        assert!(!text.is_empty());
    }

    #[test]
    fn renders_in_a_very_narrow_terminal_without_panicking() {
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(1),
            playing: true,
            quality: None,
            tier: Tier::Low,
        };
        let _ = rendered(&state, 8);
    }
    #[test]
    fn every_transport_control_is_one_cell_in_both_sets() {
        // The row is laid out on the assumption that each control takes one
        // column. A two-cell glyph -- the `▌▌` pause this used to have --
        // means padding the row to a common width, which puts the play
        // button off centre between the skips.
        let fixed = crate::shell::icons::Fixed::at(false);

        for nerd in [false, true] {
            fixed.set(nerd);
            for control in [
                crate::shell::icons::play(),
                crate::shell::icons::pause(),
                crate::shell::icons::previous(),
                crate::shell::icons::next(),
                crate::shell::icons::shuffle(),
                crate::shell::icons::repeat(),
                crate::shell::icons::repeat_one(),
            ] {
                assert_eq!(
                    control.chars().count(),
                    1,
                    "nerd={nerd}: {control:?} is not one cell, so the row shifts"
                );
            }
            // Repeat-all and repeat-one have to be told apart: the same
            // glyph for two modes says nothing about which is on.
            assert_ne!(
                crate::shell::icons::repeat(),
                crate::shell::icons::repeat_one(),
                "nerd={nerd}: the two repeat modes draw the same"
            );
        }
    }

    #[test]
    fn the_play_button_sits_midway_between_the_skips() {
        // Reported as four columns on one side and three on the other. The
        // pause used to be `▌▌`, two cells, so the row was padded to a
        // common width and given a column of lead-in to correct the ink of
        // a half block. With a one-cell pause in both sets, neither is
        // needed -- and both were what put the button off centre.
        let fixed = crate::shell::icons::Fixed::at(false);

        for nerd in [false, true] {
            fixed.set(nerd);
            for playing in [true, false] {
                let state = NowPlaying {
                    track: Some(crate::domain::Track {
                        id: crate::domain::TrackId(1),
                        title: "A Track".into(),
                        artist: "Someone".into(),
                        album: "An Album".into(),
                        duration: std::time::Duration::from_secs(200),
                        cover: None,
                        tags: Vec::new(),
                        added: None,
                        explicit: false,
            ai: false,
            radio: None,
                    }),
                    playing,
                    ..Default::default()
                };
                let buf = crate::shell::geometry::draw(
                    80,
                    crate::shell::layout::NOW_PLAYING_HEIGHT,
                    move |f, a, p| render(f, a, p, &state, Modes::default()),
                );

                let text = crate::shell::geometry::text(&buf);
                let row = text
                    .lines()
                    .find(|l| l.contains(crate::shell::icons::previous()))
                    .expect("the transport row");
                // In columns rather than bytes: the glyphs are different
                // lengths in UTF-8, and `find` returns a byte index.
                let col = |needle: &str| {
                    row.find(needle)
                        .map(|b| row[..b].chars().count())
                        .expect("a control is missing from the row")
                };
                let prev = col(crate::shell::icons::previous());
                let next = col(crate::shell::icons::next());
                let mark = col(if playing {
                    crate::shell::icons::play()
                } else {
                    crate::shell::icons::pause()
                });

                assert_eq!(
                    mark - prev - 1,
                    next - mark - 1,
                    "nerd={nerd} playing={playing}: {} columns to the left of \
                     the button and {} to the right:\n{row}",
                    mark - prev - 1,
                    next - mark - 1
                );
            }
        }
    }

}
