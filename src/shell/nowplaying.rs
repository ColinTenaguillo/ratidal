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
fn find_progress(buf: &ratatui::buffer::Buffer, palette: &Palette) -> Option<super::geometry::Span> {
    super::geometry::longest_run_coloured(buf, &[palette.text, palette.track])
}

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
    Rect { y: area.y + 1, height: area.height - 2, ..area }
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

pub fn render(frame: &mut Frame, area: Rect, palette: &Palette, state: &NowPlaying) {
    render_with_cover(frame, area, palette, state, |_, _, _, _| false)
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
    let left_width = (inner.width / 4).max(20).min(inner.width.saturating_sub(right_width));
    let left = Rect { width: left_width, ..inner };
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
    render_transport(frame, centre, inner, palette, state, track);
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
    // Square: cells are about twice as tall as they are wide.
    let thumb_w = (thumb_h * 2).min(area.width / 3);
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
            Rect { x: text_x, y, width: text_w, height: 1 },
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
    track: &Track,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // The bar keeps a blank row top and bottom, so its content sits in the
    // band between them — anchoring to `area.y` would put this in the margin.
    let band = content_band(area);

    // Shuffle, previous, play/pause, next, repeat — the same five the web
    // client shows, in the same order.
    let play = if state.playing { "▶" } else { "⏸" };
    let transport = format!("⤨   ⏮   {play}   ⏭   ⟳");
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(transport, palette.title()))
            .alignment(ratatui::layout::Alignment::Center),
        Rect { y: band.y, height: 1, ..full },
    );

    // The times sit either side of the bar rather than inside it, which is
    // what the web client does and what leaves the bar unbroken.
    let y = band.y + 1;
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
    let bar_w = if full.width % 2 == bar_w % 2 { bar_w } else { bar_w.saturating_sub(1) };
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
        Rect { x: elapsed_x, y, width: time_w, height: 1 },
    );
    render_progress(
        frame,
        Rect { x: bar_x, y, width: bar_w, height: 1 },
        palette,
        progress(state.position, track.duration),
    );
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(total, palette.subtitle())),
        Rect { x: bar_x + bar_w + TIME_GUTTER, y, width: time_w, height: 1 },
    );
}

/// The progress bar.
///
/// Drawn with box-drawing rules rather than block elements. A block sits at
/// the bottom of its cell, so against the times either side of it the bar
/// read as an underline below them rather than a rule between them — the
/// two were on the same row and still looked unaligned. `─` and `━` are
/// painted through the middle of the cell, level with the digits.
///
/// One glyph for both halves, told apart by colour alone.
///
/// `▬` rather than the heavy rule `━`: it is thicker, and unlike the block
/// elements it is drawn through the middle of the cell, so it stays level
/// with the times either side instead of dropping to the baseline.
///
/// The web client draws the track and the fill at the same 4px height — the
/// played part is not thicker, only brighter. Using heavy for the fill and
/// light for the track made the groove visibly thinner than the thing
/// running along it, as though the bar changed height at the play head.
///
/// The track shares its glyph with the bar's own top border, which is why
/// it has a colour of its own — darker than the frame, so the two rules do
/// not read as the same thing.
fn render_progress(frame: &mut Frame, area: Rect, palette: &Palette, ratio: f64) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let filled = (ratio.clamp(0.0, 1.0) * area.width as f64).round() as u16;

    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(
            "▬".repeat(area.width as usize),
            Style::default().fg(palette.track),
        )),
        area,
    );
    if filled > 0 {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                "▬".repeat(filled.min(area.width) as usize),
                Style::default().fg(palette.text),
            )),
            Rect { width: filled.min(area.width), ..area },
        );
    }
}

/// The queue and volume marks, and the delivered-quality badge.
fn render_badges(frame: &mut Frame, area: Rect, palette: &Palette, state: &NowPlaying) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Level with the transport, which the web client hangs the badge beside
    // rather than under.
    let y = content_band(area).y;
    let Some(quality) = &state.quality else { return };

    let badge = format!(" {quality} ");
    let badge_w = (badge.chars().count() as u16).min(area.width);
    let marks = "≣  ◍";
    let marks_w = marks.chars().count() as u16;

    let badge_x = area.x + area.width - badge_w;
    if marks_w + 2 <= badge_x.saturating_sub(area.x) {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(marks, palette.subtitle())),
            Rect { x: badge_x - marks_w - 2, y, width: marks_w, height: 1 },
        );
    }

    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(
            badge,
            palette.quality_badge(state.tier),
        )),
        Rect { x: badge_x, y, width: badge_w, height: 1 },
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

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
        geometry::draw(width, crate::shell::layout::NOW_PLAYING_HEIGHT, move |f, area, p| {
            render(f, area, p, &state)
        })
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

        assert_eq!(elapsed.row, run.row, "the elapsed time shares the bar's row");
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
        terminal.draw(|frame| render(frame, frame.area(), &palette, state)).unwrap();
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
        assert_eq!(Tier::of_quality(Quality::Lossless, Some(24), 44_100), Tier::Max);
        assert_eq!(Tier::of_quality(Quality::Lossless, Some(16), 96_000), Tier::Max);
        assert_eq!(Tier::of_quality(Quality::HiResLossless, Some(24), 44_100), Tier::Max);
    }

    #[test]
    fn the_tiers_match_what_tidal_shows_for_the_same_stream() {
        // Checked against the web client with a track playing: it labels
        // 24-bit 44.1kHz MAX in amber, not HIGH. Bit depth alone earns the
        // top badge; the sample rate does not have to exceed CD.
        assert_eq!(Tier::of(Some(24), 44_100), Tier::Max, "TIDAL calls this MAX");
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
        assert_eq!(Tier::of(Some(24), 44_100), Tier::Max, "24-bit at CD rate is still hi-res");
        assert_eq!(Tier::of(Some(16), 96_000), Tier::Max, "above CD rate is hi-res");

        assert_eq!(Tier::of(Some(16), 44_100), Tier::High, "CD-quality lossless");
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
        assert_eq!(progress(Duration::from_secs(10), Duration::from_secs(5)), 1.0);
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
        assert!(text.contains("Money Trees"), "title lost to the thumb:\n{text}");
        assert!(text.contains("Kendrick"), "artist lost to the thumb:\n{text}");
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
}
