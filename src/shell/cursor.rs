//! The one way a selection moves and stays on screen.
//!
//! Every list, grid and row of cards keeps a selected index and an offset
//! scrolled past. Each used to move them with its own copy of the same
//! three rules, and the copies drifted: one saturated where another
//! overflowed, one scrolled against the cut row at the fold where another
//! landed on it. These are the rules, once.

/// Step `selected` `by` places down or up, clamped to the `len` items.
///
/// `len` only matters going down: up stops at the first item whatever the
/// list holds, so a caller moving up may pass nothing for it.
pub fn step(selected: &mut usize, down: bool, by: usize, len: usize) {
    *selected = if !down {
        selected.saturating_sub(by)
    } else if len == 0 {
        0
    } else {
        selected.saturating_add(by).min(len - 1)
    };
}

/// Pull `offset` just far enough that `at` is within the `visible` items
/// drawn from it.
///
/// Saturating throughout: both are indices the caller hands in, and a row
/// that shrank under a selection near its end once added past the top of a
/// usize here and panicked.
pub fn scroll_into_view(offset: &mut usize, at: usize, visible: usize) {
    let visible = visible.max(1);
    if at < *offset {
        *offset = at;
    } else if at >= offset.saturating_add(visible) {
        *offset = at.saturating_add(1).saturating_sub(visible);
    }
}

/// Keep a selection inside a list that may have shrunk under it — the
/// filter box does exactly that on every keystroke.
pub fn clamp(selected: &mut usize, offset: &mut usize, len: usize) {
    if len == 0 {
        *selected = 0;
        *offset = 0;
    } else if *selected >= len {
        *selected = len - 1;
    }
}

/// How many items `item` long, `gap` apart, `space` holds, counting the one
/// cut at the edge, and whether that last one is cut.
///
/// The pane's edge cuts the way a browser's does: whatever is left after
/// the whole items is drawn, so it counts, and the keys must be able to
/// reach it. Selecting it scrolls it in whole, which is what `landable`
/// is for.
pub fn fit(space: u16, item: u16, gap: u16) -> (usize, bool) {
    let step = item + gap;
    let whole = (space.saturating_add(gap) / step.max(1)) as usize;
    let left = space.saturating_sub(whole as u16 * step);
    (whole + usize::from(left > 0), left > 0)
}

/// Of `count` items drawn, the ones the selection may land on without
/// scrolling: all of them but the cut one at the edge.
pub fn landable((count, cut): (usize, bool)) -> usize {
    count.saturating_sub(usize::from(cut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_clamps_at_both_ends_and_survives_an_empty_list() {
        let mut s = 8;
        step(&mut s, true, 3, 10);
        assert_eq!(s, 9, "clamped to the last");
        step(&mut s, false, 20, 10);
        assert_eq!(s, 0, "and to the first");
        step(&mut s, true, 1, 0);
        assert_eq!(s, 0, "an empty list has nothing to select");
        s = 4;
        step(&mut s, false, 1, 0);
        assert_eq!(s, 3, "up needs no length");
    }

    #[test]
    fn scrolling_moves_the_window_only_as_far_as_it_must() {
        let mut off = 0;
        scroll_into_view(&mut off, 5, 3);
        assert_eq!(off, 3, "5 lands on the last of three");
        scroll_into_view(&mut off, 4, 3);
        assert_eq!(off, 3, "already on screen: nothing moves");
        scroll_into_view(&mut off, 1, 3);
        assert_eq!(off, 1, "back up to it");
        scroll_into_view(&mut off, usize::MAX, 3);
        assert!(off > 0, "an index at the top of a usize does not panic");
    }

    #[test]
    fn fitting_counts_the_cut_item_and_says_so() {
        // 9-tall items, one blank between: two whole take 19.
        assert_eq!(fit(19, 9, 1), (2, false));
        assert_eq!(fit(20, 9, 1), (2, false), "the gap alone is not an item");
        assert_eq!(fit(21, 9, 1), (3, true), "one line of the next is");
        assert_eq!(fit(0, 9, 1), (0, false));
        assert_eq!(landable((3, true)), 2);
        assert_eq!(landable((2, false)), 2);
        assert_eq!(landable((1, true)), 0, "one cut item is nothing to land on");
    }
}
