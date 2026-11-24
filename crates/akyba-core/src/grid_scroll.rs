//! The grid wider than the window: how wide it gets when every pane keeps a readable width, where
//! each pane sits across it, and which panes are out of sight on either side. Distances are in
//! pixels; `scroll` is how far the grid has moved left.

/// A pane's place across the grid, from its left edge to its right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub left: f32,
    pub right: f32,
}

impl Span {
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    fn center(self) -> f32 {
        f32::midpoint(self.left, self.right)
    }
}

/// Which side of the window a pane is hidden past.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// How wide the grid is: the window's width, or wider when the busiest row's panes would get
/// narrower than `narrowest`. A `narrowest` of zero squeezes every pane into the window, as Akyba
/// did before the grid scrolled.
#[expect(clippy::cast_precision_loss, reason = "a handful of panes")]
pub fn content_width(viewport: f32, panes_across: usize, narrowest: f32, gap: f32) -> f32 {
    if narrowest <= 0. || panes_across == 0 {
        return viewport;
    }
    let needed = narrowest * panes_across as f32 + gap * (panes_across - 1) as f32;
    viewport.max(needed)
}

/// Where a row's panes sit, laid out as the grid lays them out: the gaps between them are fixed,
/// and what's left is shared by their relative sizes.
#[expect(clippy::cast_precision_loss, reason = "a handful of panes")]
pub fn spans(sizes: &[f32], width: f32, gap: f32) -> Vec<Span> {
    let total: f32 = sizes.iter().sum();
    if sizes.is_empty() || total <= 0. {
        return Vec::new();
    }
    let shared = (width - gap * (sizes.len() - 1) as f32).max(0.);
    let mut left = 0.;
    sizes
        .iter()
        .map(|size| {
            let span = Span { left, right: left + shared * size / total };
            left = span.right + gap;
            span
        })
        .collect()
}

/// The side a pane is hidden on, or none while at least half of it shows: a pane half in sight
/// lights its own ring, so it needs no sign at the edge.
pub fn hidden_side(span: Span, scroll: f32, viewport: f32) -> Option<Side> {
    let shown = (span.right.min(scroll + viewport) - span.left.max(scroll)).max(0.);
    if shown * 2. >= span.width() {
        return None;
    }
    Some(if span.center() < scroll + viewport / 2. { Side::Left } else { Side::Right })
}

/// How far to scroll for a pane to be wholly in sight, moving as little as it can; none when it
/// already is. A pane wider than the window shows from its left edge.
pub fn reveal(span: Span, scroll: f32, viewport: f32, max: f32) -> Option<f32> {
    reveal_on_loop(span, scroll, viewport).map(|to| to.clamp(0., max))
}

/// As `reveal`, on a grid that loops: no end to stop at, so the answer can be anywhere along it.
pub fn reveal_on_loop(span: Span, scroll: f32, viewport: f32) -> Option<f32> {
    let in_sight = span.left >= scroll - 0.5 && span.right <= scroll + viewport + 0.5;
    if in_sight {
        return None;
    }
    Some(if span.left < scroll || span.width() > viewport { span.left } else { span.right - viewport })
}

/// Whether the grid can loop, the first pane coming round again after the last. Each row is drawn
/// once, starting with the pane at the window's left edge, so even a row that starts just inside
/// its widest pane must still reach the window's right edge.
pub fn loops(width: f32, viewport: f32, widest: f32, gap: f32) -> bool {
    viewport > 0. && width - widest - gap >= viewport
}

/// How far the grid goes round before it repeats: its width and the gap at the seam.
pub fn ring(width: f32, gap: f32) -> f32 {
    width + gap
}

/// Where a looping row starts drawing: the pane at the window's left edge (each pane owning the gap
/// after it, the last one the seam's), and how far the window starts past that pane's left edge.
/// `phase` is how far round the ring the window is, from zero up to the ring's length.
pub fn row_start(spans: &[Span], phase: f32, gap: f32) -> (usize, f32) {
    let index = spans.iter().position(|span| phase < span.right + gap).unwrap_or(spans.len().saturating_sub(1));
    (index, spans.get(index).map_or(0., |span| phase - span.left))
}

/// The copy of a pane on a looping grid nearest the middle of the window: the one to show, or to
/// call the agent in from the nearer side.
pub fn nearest_copy(span: Span, scroll: f32, viewport: f32, ring: f32) -> Span {
    if ring <= 0. {
        return span;
    }
    let turns = ((scroll + viewport / 2. - span.center()) / ring).round();
    Span { left: span.left + turns * ring, right: span.right + turns * ring }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_that_fit_keep_the_grid_as_wide_as_the_window() {
        assert!((content_width(1600., 3, 500., 4.) - 1600.).abs() < 0.01);
        assert!((content_width(1600., 6, 0., 4.) - 1600.).abs() < 0.01);
    }

    #[test]
    fn panes_that_would_be_too_narrow_widen_the_grid() {
        assert!((content_width(1600., 6, 500., 4.) - 3020.).abs() < 0.01);
    }

    #[test]
    fn spans_share_the_width_by_size_around_fixed_gaps() {
        let spans = spans(&[1., 1., 2.], 412., 4.);
        assert_eq!(spans.len(), 3);
        assert!((spans[0].left).abs() < 0.01 && (spans[0].right - 101.).abs() < 0.01);
        assert!((spans[1].left - 105.).abs() < 0.01);
        assert!((spans[2].right - 412.).abs() < 0.01);
    }

    #[test]
    fn a_pane_counts_as_hidden_once_less_than_half_shows() {
        let pane = Span { left: 1000., right: 1500. };
        assert_eq!(hidden_side(pane, 0., 1200.), Some(Side::Right));
        assert_eq!(hidden_side(pane, 100., 1200.), None);
        let first = Span { left: 0., right: 500. };
        assert_eq!(hidden_side(first, 300., 1200.), Some(Side::Left));
        assert_eq!(hidden_side(first, 200., 1200.), None);
    }

    #[test]
    fn revealing_a_pane_moves_as_little_as_it_can() {
        let pane = Span { left: 1000., right: 1500. };
        assert_eq!(reveal(pane, 400., 1200., 2000.), None);
        assert_eq!(reveal(pane, 0., 1200., 2000.), Some(300.));
        assert_eq!(reveal(pane, 1200., 1200., 2000.), Some(1000.));
    }

    #[test]
    fn a_grid_loops_only_when_every_row_still_covers_the_window() {
        assert!(loops(2000., 1200., 500., 4.));
        assert!(!loops(1600., 1200., 500., 4.));
        assert!(!loops(2000., 0., 500., 4.));
    }

    #[test]
    fn a_looping_row_starts_with_the_pane_at_the_left_edge() {
        let spans = spans(&[1., 1., 1.], 1508., 4.);
        // Panes at 0..500, 504..1004, 1008..1508; the ring is 1512 long.
        assert_eq!(row_start(&spans, 0., 4.), (0, 0.));
        assert_eq!(row_start(&spans, 502., 4.), (0, 502.));
        assert_eq!(row_start(&spans, 600., 4.), (1, 96.));
        assert_eq!(row_start(&spans, 1509., 4.), (2, 501.));
    }

    #[test]
    fn the_nearest_copy_is_the_one_closest_to_the_window() {
        let first = Span { left: 0., right: 500. };
        let near = nearest_copy(first, 1400., 1000., 2000.);
        assert!((near.left - 2000.).abs() < 0.01);
        assert_eq!(nearest_copy(first, 0., 1000., 2000.), first);
    }

    #[test]
    fn revealing_on_a_loop_never_stops_at_an_end() {
        let pane = Span { left: -600., right: -100. };
        assert_eq!(reveal_on_loop(pane, 0., 1200.), Some(-600.));
    }
}
