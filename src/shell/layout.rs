use ratatui::layout::Rect;

pub const SIDEBAR_WIDTH: u16 = 26;
/// Border, then five rows of content.
///
/// The web client's bar is 88px with a 52px cover and 18px of clear space
/// above and below it — 59% cover, the rest margin. Five rows is the
/// shortest that divides that way on a character grid: one blank, three of
/// cover, one blank. Four rows can only give 50% or none at all.
pub const NOW_PLAYING_HEIGHT: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    pub sidebar: Rect,
    pub main: Rect,
    pub now_playing: Rect,
}

/// Split the frame into the three regions of the web layout: a fixed-width
/// sidebar, the main area, and a fixed-height bar across the bottom.
///
/// All arithmetic saturates: `Rect` uses `u16`, so a terminal smaller than the
/// fixed sizes would otherwise underflow.
pub fn split(area: Rect) -> Regions {
    let bar_height = NOW_PLAYING_HEIGHT.min(area.height);
    let upper_height = area.height.saturating_sub(bar_height);
    let sidebar_width = SIDEBAR_WIDTH.min(area.width);

    Regions {
        sidebar: Rect {
            x: area.x,
            y: area.y,
            width: sidebar_width,
            height: upper_height,
        },
        main: Rect {
            x: area.x + sidebar_width,
            y: area.y,
            width: area.width.saturating_sub(sidebar_width),
            height: upper_height,
        },
        now_playing: Rect {
            x: area.x,
            y: area.y + upper_height,
            width: area.width,
            height: bar_height,
        },
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;

    #[test]
    fn splits_into_sidebar_main_and_now_playing() {
        let r = split(Rect::new(0, 0, 120, 40));

        // Sidebar is a fixed 26 columns, matching the web client's proportions.
        assert_eq!(r.sidebar.width, 26);
        assert_eq!(r.sidebar.x, 0);

        // The now-playing bar is a fixed 6 rows at the bottom, full width:
        // a border, then five of content — a blank row, three of cover and
        // track detail, and a blank row, which is how the web client's 18px
        // margins around a 52px cover come out on a character grid.
        assert_eq!(r.now_playing.height, NOW_PLAYING_HEIGHT);
        assert_eq!(NOW_PLAYING_HEIGHT, 6);
        assert_eq!(r.now_playing.width, 120);
        assert_eq!(r.now_playing.y, 40 - NOW_PLAYING_HEIGHT);

        // Main takes the remaining width, beside the sidebar, and whatever
        // height the bar leaves.
        assert_eq!(r.main.x, 26);
        assert_eq!(r.main.width, 94);
        assert_eq!(r.main.height, 40 - NOW_PLAYING_HEIGHT);
    }

    #[test]
    fn regions_never_overlap() {
        let r = split(Rect::new(0, 0, 100, 30));
        assert_eq!(r.sidebar.right(), r.main.x, "sidebar and main must abut");
        assert_eq!(r.sidebar.bottom(), r.now_playing.y);
        assert_eq!(r.main.bottom(), r.now_playing.y);
    }

    #[test]
    fn a_terminal_too_short_for_the_bar_does_not_panic() {
        // Rect arithmetic on u16 underflows if written carelessly; a tiny
        // terminal must degrade, not crash.
        let r = split(Rect::new(0, 0, 20, 3));
        assert!(r.now_playing.height <= 3);
        assert_eq!(r.sidebar.height + r.now_playing.height, 3);
    }

    #[test]
    fn a_terminal_narrower_than_the_sidebar_does_not_panic() {
        let r = split(Rect::new(0, 0, 10, 20));
        assert!(r.sidebar.width <= 10);
        assert_eq!(r.main.width, 10 - r.sidebar.width);
    }
}
