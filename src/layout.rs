use ratatui::layout::Rect;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Split {
    Vertical,
    Horizontal,
}

/// Which pane a leaf holds.
pub type PaneId = usize;

/// How a window is carved up. Splitting replaces one leaf with a pair, so a
/// split only ever divides the pane you are looking at - the rest of the
/// window keeps its shape, exactly as tmux does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Leaf(PaneId),
    Split {
        dir: Split,
        /// Share of the space given to `first`, as a percentage.
        ratio: u16,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    pub fn leaf(id: PaneId) -> Node {
        Node::Leaf(id)
    }

    /// Panes in layout order.
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { first, second, .. } => {
                let mut v = first.leaves();
                v.extend(second.leaves());
                v
            }
        }
    }

    /// Splits `target` in two, putting `new_id` after it. Returns false if the
    /// pane is not in this tree.
    pub fn split_leaf(&mut self, target: PaneId, dir: Split, new_id: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                *self = Node::Split {
                    dir,
                    ratio: EVEN,
                    first: Box::new(Node::Leaf(target)),
                    second: Box::new(Node::Leaf(new_id)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                first.split_leaf(target, dir, new_id) || second.split_leaf(target, dir, new_id)
            }
        }
    }

    /// Drops a pane, collapsing its parent into the surviving sibling.
    /// Returns None when the tree had nothing else left.
    pub fn remove_leaf(self, target: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(id) if id == target => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split {
                dir,
                ratio,
                first,
                second,
            } => match (first.remove_leaf(target), second.remove_leaf(target)) {
                (None, None) => None,
                (Some(only), None) | (None, Some(only)) => Some(only),
                (Some(a), Some(b)) => Some(Node::Split {
                    dir,
                    ratio,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
            },
        }
    }

    /// Where every pane sits inside `area`.
    pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            Node::Leaf(id) => vec![(*id, area)],
            Node::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let (a, b) = divide(area, *dir, *ratio);
                let mut out = first.rects(a);
                out.extend(second.rects(b));
                out
            }
        }
    }
}

/// An even split, and how far a ratio may be pushed either way.
pub const EVEN: u16 = 50;
pub const MIN_RATIO: u16 = 10;
pub const MAX_RATIO: u16 = 90;
/// How much one resize keystroke moves a border.
pub const RESIZE_STEP: u16 = 5;

/// How many of `total` cells the first half gets. Always leaves a cell for
/// the second half, so a pane never vanishes.
fn share(total: u16, ratio: u16) -> u16 {
    if total < 2 {
        return total;
    }
    ((total as u32 * ratio as u32).div_ceil(100) as u16).clamp(1, total - 1)
}

/// Divides a rect in two, giving `ratio` percent to the first half. Each half
/// always keeps at least one cell so a pane never vanishes.
fn divide(area: Rect, dir: Split, ratio: u16) -> (Rect, Rect) {
    match dir {
        Split::Vertical => {
            let left = share(area.width, ratio);
            (
                Rect::new(area.x, area.y, left, area.height),
                Rect::new(area.x + left, area.y, area.width - left, area.height),
            )
        }
        Split::Horizontal => {
            let top = share(area.height, ratio);
            (
                Rect::new(area.x, area.y, area.width, top),
                Rect::new(area.x, area.y + top, area.width, area.height - top),
            )
        }
    }
}

impl Node {
    /// Moves the border `target` sits against, in `dir`.
    ///
    /// The nearest enclosing split on that axis is the one that moves. A pane
    /// before the border grows as the border moves away; a pane after it
    /// shrinks. Phrased this way every key always does something and every
    /// resize can be undone from the same pane.
    pub fn resize(&mut self, target: PaneId, dir: Dir, step: u16) -> bool {
        let Node::Split {
            dir: axis,
            ratio,
            first,
            second,
        } = self
        else {
            return false;
        };

        // Splits closer to the pane win.
        if first.resize(target, dir, step) || second.resize(target, dir, step) {
            return true;
        }

        let on_axis = matches!(
            (*axis, dir),
            (Split::Vertical, Dir::Left | Dir::Right) | (Split::Horizontal, Dir::Up | Dir::Down)
        );
        if !on_axis {
            return false;
        }

        if !first.leaves().contains(&target) && !second.leaves().contains(&target) {
            return false;
        }
        let delta = match dir {
            Dir::Right | Dir::Down => step as i32,
            Dir::Left | Dir::Up => -(step as i32),
        };

        let next = (*ratio as i32 + delta).clamp(MIN_RATIO as i32, MAX_RATIO as i32) as u16;
        if next == *ratio {
            return true; // at the stop, but this border was the right one
        }
        *ratio = next;
        true
    }
}

/// Which half of a split to descend into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    First,
    Second,
}

/// A route from the root of the tree down to one split. A border keeps its
/// identity this way, so a mouse drag goes on moving the same border even
/// as the layout around it is redrawn.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Path(pub Vec<Side>);

/// A line between panes, and the split that owns it. This is what the mouse
/// grabs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Border {
    pub path: Path,
    /// Vertical means a vertical line, dragged sideways.
    pub dir: Split,
    /// The column (or row) the line is drawn at: the first cell of the
    /// second half.
    pub pos: u16,
    /// How far the line runs, as `start..end` across the other axis.
    pub span: (u16, u16),
    /// The region the owning split divides, which is what a ratio is of.
    pub area: Rect,
}

impl Border {
    /// True when the pointer is on this line. Both panes draw a border where
    /// they meet, so the cell before the line grabs it too.
    pub fn hits(&self, x: u16, y: u16) -> bool {
        let (along, across) = match self.dir {
            Split::Vertical => (y, x),
            Split::Horizontal => (x, y),
        };
        along >= self.span.0
            && along < self.span.1
            && (across == self.pos || across + 1 == self.pos)
    }

    /// The ratio that puts this border where the pointer is.
    pub fn ratio_at(&self, coord: u16) -> u16 {
        let (start, total) = match self.dir {
            Split::Vertical => (self.area.x, self.area.width),
            Split::Horizontal => (self.area.y, self.area.height),
        };
        let want = coord.saturating_sub(start).min(total) as i32;
        // One percent can be worth more than one cell, so take whichever
        // ratio lands the border nearest the pointer rather than computing
        // one that cannot be drawn.
        (MIN_RATIO..=MAX_RATIO)
            .min_by_key(|r| (share(total, *r) as i32 - want).abs())
            .unwrap_or(EVEN)
    }

    /// The pointer coordinate that matters for this border's axis.
    pub fn coord(&self, x: u16, y: u16) -> u16 {
        match self.dir {
            Split::Vertical => x,
            Split::Horizontal => y,
        }
    }
}

impl Node {
    /// Every border in the tree, outermost first.
    pub fn borders(&self, area: Rect) -> Vec<Border> {
        let mut out = Vec::new();
        self.collect_borders(area, &mut Vec::new(), &mut out);
        out
    }

    fn collect_borders(&self, area: Rect, at: &mut Vec<Side>, out: &mut Vec<Border>) {
        let Node::Split {
            dir,
            ratio,
            first,
            second,
        } = self
        else {
            return;
        };
        let (a, b) = divide(area, *dir, *ratio);
        out.push(Border {
            path: Path(at.clone()),
            dir: *dir,
            pos: match dir {
                Split::Vertical => b.x,
                Split::Horizontal => b.y,
            },
            span: match dir {
                Split::Vertical => (area.y, area.y + area.height),
                Split::Horizontal => (area.x, area.x + area.width),
            },
            area,
        });
        at.push(Side::First);
        first.collect_borders(a, at, out);
        at.pop();
        at.push(Side::Second);
        second.collect_borders(b, at, out);
        at.pop();
    }

    /// Moves one named border, as a mouse drag does. Returns false if the
    /// path no longer leads to a split.
    pub fn set_ratio(&mut self, path: &Path, ratio: u16) -> bool {
        let mut node = self;
        for step in &path.0 {
            let Node::Split { first, second, .. } = node else {
                return false;
            };
            node = match step {
                Side::First => first,
                Side::Second => second,
            };
        }
        let Node::Split { ratio: r, .. } = node else {
            return false;
        };
        *r = ratio.clamp(MIN_RATIO, MAX_RATIO);
        true
    }
}

/// The pane to move to from `from` heading `dir`, picking the nearest one
/// whose edge faces you and whose span overlaps where you already are.
pub fn neighbour(rects: &[(PaneId, Rect)], from: PaneId, dir: Dir) -> Option<PaneId> {
    let me = rects.iter().find(|(id, _)| *id == from)?.1;
    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, r)| match dir {
            Dir::Left => r.x + r.width <= me.x,
            Dir::Right => r.x >= me.x + me.width,
            Dir::Up => r.y + r.height <= me.y,
            Dir::Down => r.y >= me.y + me.height,
        })
        .filter(|(_, r)| match dir {
            // must actually be alongside, not diagonally off
            Dir::Left | Dir::Right => r.y < me.y + me.height && me.y < r.y + r.height,
            Dir::Up | Dir::Down => r.x < me.x + me.width && me.x < r.x + r.width,
        })
        .min_by_key(|(_, r)| match dir {
            Dir::Left => me.x - (r.x + r.width),
            Dir::Right => r.x - (me.x + me.width),
            Dir::Up => me.y - (r.y + r.height),
            Dir::Down => r.y - (me.y + me.height),
        })
        .map(|(id, _)| *id)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}
#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 80, 24)
    }

    fn rect_of(n: &Node, id: PaneId, a: Rect) -> Rect {
        n.rects(a).into_iter().find(|(p, _)| *p == id).unwrap().1
    }

    #[test]
    fn one_pane_fills_the_window() {
        let n = Node::leaf(1);
        assert_eq!(n.rects(area()), vec![(1, area())]);
        assert_eq!(n.leaves(), vec![1]);
    }

    #[test]
    fn a_vertical_split_puts_panes_side_by_side() {
        let mut n = Node::leaf(1);
        assert!(n.split_leaf(1, Split::Vertical, 2));
        assert_eq!(
            n.rects(area()),
            vec![(1, Rect::new(0, 0, 40, 24)), (2, Rect::new(40, 0, 40, 24))]
        );
    }

    #[test]
    fn a_horizontal_split_stacks_panes() {
        let mut n = Node::leaf(1);
        assert!(n.split_leaf(1, Split::Horizontal, 2));
        assert_eq!(
            n.rects(area()),
            vec![(1, Rect::new(0, 0, 80, 12)), (2, Rect::new(0, 12, 80, 12))]
        );
    }

    #[test]
    fn splitting_one_pane_leaves_the_others_alone() {
        // Two side by side, then split only the right-hand one.
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        let left_before = rect_of(&n, 1, area());
        assert!(n.split_leaf(2, Split::Horizontal, 3));

        assert_eq!(
            rect_of(&n, 1, area()),
            left_before,
            "the untouched pane must keep its shape"
        );
        let r2 = rect_of(&n, 2, area());
        let r3 = rect_of(&n, 3, area());
        assert_eq!(r2, Rect::new(40, 0, 40, 12));
        assert_eq!(r3, Rect::new(40, 12, 40, 12));
        assert_eq!(left_before, Rect::new(0, 0, 40, 24), "still full height");
    }

    #[test]
    fn splitting_the_left_pane_leaves_the_right_alone() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        let right_before = rect_of(&n, 2, area());
        n.split_leaf(1, Split::Horizontal, 3);
        assert_eq!(rect_of(&n, 2, area()), right_before);
        assert_eq!(rect_of(&n, 1, area()), Rect::new(0, 0, 40, 12));
        assert_eq!(rect_of(&n, 3, area()), Rect::new(0, 12, 40, 12));
    }

    #[test]
    fn splitting_an_unknown_pane_changes_nothing() {
        let mut n = Node::leaf(1);
        assert!(!n.split_leaf(99, Split::Vertical, 2));
        assert_eq!(n, Node::leaf(1));
    }

    #[test]
    fn rects_always_cover_the_whole_area() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        n.split_leaf(1, Split::Horizontal, 4);
        let a = Rect::new(0, 0, 81, 25);
        let covered: u32 = n.rects(a).iter().map(|(_, r)| r.area()).sum();
        assert_eq!(covered, a.area(), "no gaps and no overlap");
    }

    #[test]
    fn leaves_come_back_in_layout_order() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        assert_eq!(n.leaves(), vec![1, 2, 3]);
    }

    #[test]
    fn removing_a_pane_gives_its_space_to_the_sibling() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        let n = n.remove_leaf(2).unwrap();
        assert_eq!(n, Node::leaf(1));
        assert_eq!(n.rects(area()), vec![(1, area())]);
    }

    #[test]
    fn removing_collapses_only_the_relevant_split() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        let n = n.remove_leaf(3).unwrap();
        assert_eq!(n.leaves(), vec![1, 2]);
        assert_eq!(rect_of(&n, 2, area()), Rect::new(40, 0, 40, 24));
    }

    #[test]
    fn removing_the_only_pane_empties_the_tree() {
        assert_eq!(Node::leaf(1).remove_leaf(1), None);
    }

    #[test]
    fn removing_something_absent_is_a_no_op() {
        let n = Node::leaf(1).remove_leaf(9).unwrap();
        assert_eq!(n, Node::leaf(1));
    }

    // ---- manual resize ----

    #[test]
    fn a_fresh_split_is_even() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        let r = n.rects(area());
        assert_eq!(r[0].1.width, 40);
        assert_eq!(r[1].1.width, 40);
    }

    #[test]
    fn growing_the_left_pane_rightwards_moves_the_border() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        assert!(n.resize(1, Dir::Right, 10));
        let r = n.rects(area());
        assert_eq!(rect_of(&n, 1, area()).width, 48);
        assert_eq!(rect_of(&n, 2, area()).width, 32);
        assert_eq!(r.iter().map(|(_, x)| x.width).sum::<u16>(), 80, "no gap");
    }

    #[test]
    fn growing_the_right_pane_leftwards_moves_the_same_border() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        assert!(n.resize(2, Dir::Left, 10));
        assert_eq!(rect_of(&n, 2, area()).width, 48);
        assert_eq!(rect_of(&n, 1, area()).width, 32);
    }

    #[test]
    fn stacked_panes_resize_vertically() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Horizontal, 2);
        assert!(n.resize(1, Dir::Down, 25));
        assert_eq!(rect_of(&n, 1, area()).height, 18);
        assert_eq!(rect_of(&n, 2, area()).height, 6);
    }

    #[test]
    fn a_resize_can_always_be_undone_from_the_same_pane() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        let even = rect_of(&n, 1, area());

        // Focus is on the right pane: grow it, then give the space back.
        assert!(n.resize(2, Dir::Left, 10));
        assert_ne!(rect_of(&n, 1, area()), even);
        assert!(n.resize(2, Dir::Right, 10), "the key must not be dead");
        assert_eq!(rect_of(&n, 1, area()), even, "back where it started");
    }

    #[test]
    fn either_pane_can_drive_the_border_either_way() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        for pane in [1, 2] {
            for dir in [Dir::Left, Dir::Right] {
                assert!(n.resize(pane, dir, 5), "pane {pane} {dir:?} did nothing");
            }
        }
    }

    #[test]
    fn resizing_across_the_wrong_axis_does_nothing() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        assert!(!n.resize(1, Dir::Up, 10));
        assert!(!n.resize(1, Dir::Down, 10));
    }

    #[test]
    fn the_nearest_border_is_the_one_that_moves() {
        // 1 on the left; 2 and 3 stacked on the right
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        let left_before = rect_of(&n, 1, area());

        assert!(n.resize(2, Dir::Down, 25));
        assert_eq!(rect_of(&n, 1, area()), left_before, "the outer split held");
        assert_eq!(rect_of(&n, 2, area()).height, 18);
        assert_eq!(rect_of(&n, 3, area()).height, 6);
    }

    #[test]
    fn the_search_walks_outwards_when_the_near_border_cannot_help() {
        // 2 is the top-right pane; growing it left has to move the outer
        // vertical border, not the horizontal one it sits against.
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        assert!(n.resize(2, Dir::Left, 10));
        assert_eq!(rect_of(&n, 1, area()).width, 32);
        assert_eq!(rect_of(&n, 2, area()).width, 48);
        assert_eq!(rect_of(&n, 3, area()).width, 48, "its sibling came with it");
    }

    #[test]
    fn a_border_stops_rather_than_crushing_a_pane() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        for _ in 0..50 {
            n.resize(1, Dir::Right, 10);
        }
        let r2 = rect_of(&n, 2, area());
        assert!(r2.width >= 1, "a pane must never vanish: {r2:?}");
        assert_eq!(rect_of(&n, 1, area()).width + r2.width, 80);
    }

    #[test]
    fn resizing_an_unknown_pane_changes_nothing() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        assert!(!n.resize(99, Dir::Right, 10));
        assert_eq!(rect_of(&n, 1, area()).width, 40);
    }

    #[test]
    fn a_resized_layout_still_covers_everything() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        n.resize(1, Dir::Right, 15);
        n.resize(2, Dir::Down, 20);
        let a = Rect::new(0, 0, 81, 25);
        let covered: u32 = n.rects(a).iter().map(|(_, r)| r.area()).sum();
        assert_eq!(covered, a.area(), "no gaps and no overlap");
    }

    #[test]
    fn removing_a_pane_forgets_its_border() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.resize(1, Dir::Right, 20);
        let n = n.remove_leaf(2).unwrap();
        assert_eq!(
            n.rects(area()),
            vec![(1, area())],
            "the survivor takes it all"
        );
    }

    #[test]
    fn neighbours_are_found_by_geometry() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n.split_leaf(2, Split::Horizontal, 3);
        let r = n.rects(area());
        // layout: 1 on the left full height, 2 top right, 3 bottom right
        assert_eq!(neighbour(&r, 1, Dir::Right), Some(2));
        assert_eq!(neighbour(&r, 2, Dir::Left), Some(1));
        assert_eq!(neighbour(&r, 3, Dir::Left), Some(1));
        assert_eq!(neighbour(&r, 2, Dir::Down), Some(3));
        assert_eq!(neighbour(&r, 3, Dir::Up), Some(2));
    }

    #[test]
    fn there_is_no_neighbour_past_the_edge() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        let r = n.rects(area());
        assert_eq!(neighbour(&r, 1, Dir::Left), None);
        assert_eq!(neighbour(&r, 2, Dir::Right), None);
        assert_eq!(neighbour(&r, 1, Dir::Up), None);
        assert_eq!(neighbour(&r, 1, Dir::Down), None);
    }

    #[test]
    fn a_pane_that_is_only_diagonally_away_is_not_a_neighbour() {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Horizontal, 2); // 1 top, 2 bottom
        n.split_leaf(1, Split::Vertical, 3); // top row: 1 left, 3 right
        let r = n.rects(area());
        // 2 is the whole bottom row, so it is below both 1 and 3
        assert_eq!(neighbour(&r, 3, Dir::Down), Some(2));
        assert_eq!(neighbour(&r, 1, Dir::Down), Some(2));
        assert_eq!(neighbour(&r, 2, Dir::Up), Some(1), "leftmost of the row");
    }

    // ---- mouse drag ----

    fn vsplit() -> Node {
        let mut n = Node::leaf(1);
        n.split_leaf(1, Split::Vertical, 2);
        n
    }

    /// 1 on the left, 2 above 3 on the right.
    fn nested() -> Node {
        let mut n = vsplit();
        n.split_leaf(2, Split::Horizontal, 3);
        n
    }

    #[test]
    fn a_single_pane_has_no_borders() {
        assert!(Node::leaf(1).borders(area()).is_empty());
    }

    #[test]
    fn a_border_sits_where_two_panes_meet() {
        let b = vsplit().borders(area());
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].dir, Split::Vertical);
        assert_eq!(b[0].pos, 40, "the first column of the right-hand pane");
        assert_eq!(b[0].span, (0, 24), "it runs the full height");
        assert_eq!(b[0].path, Path(vec![]), "the root split owns it");
    }

    #[test]
    fn every_split_contributes_its_own_border() {
        let b = nested().borders(area());
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].dir, b[0].pos), (Split::Vertical, 40));
        assert_eq!((b[1].dir, b[1].pos), (Split::Horizontal, 12));
        assert_eq!(b[1].span, (40, 80), "only across the right-hand column");
        assert_eq!(b[1].path, Path(vec![Side::Second]));
    }

    #[test]
    fn a_border_is_grabbed_from_either_side_of_the_line() {
        let b = &vsplit().borders(area())[0];
        assert!(b.hits(40, 5), "the line itself");
        assert!(b.hits(39, 5), "the left pane draws a border here too");
        assert!(!b.hits(38, 5));
        assert!(!b.hits(41, 5));
    }

    #[test]
    fn a_border_is_not_grabbed_beyond_where_it_is_drawn() {
        let b = nested().borders(area());
        let inner = b.iter().find(|b| b.dir == Split::Horizontal).unwrap();
        assert!(inner.hits(60, 12), "inside the right-hand column");
        assert!(!inner.hits(10, 12), "the left pane has no such border");
    }

    #[test]
    fn dragging_a_border_takes_it_to_the_pointer() {
        let mut n = vsplit();
        let b = n.borders(area())[0].clone();
        assert!(n.set_ratio(&b.path, b.ratio_at(60)));
        assert_eq!(n.borders(area())[0].pos, 60, "it followed the pointer");
        assert_eq!(rect_of(&n, 1, area()).width, 60);
        assert_eq!(rect_of(&n, 2, area()).width, 20);
    }

    #[test]
    fn a_drag_tracks_the_pointer_at_any_window_size() {
        // A whole percent is more than a cell on a wide window, so the
        // border can only ever get within one cell of the pointer.
        for width in [40, 80, 100, 137, 160, 203] {
            let a = Rect::new(0, 0, width, 24);
            let mut n = vsplit();
            for coord in (width / 8)..(width - width / 8) {
                let b = n.borders(a)[0].clone();
                n.set_ratio(&b.path, b.ratio_at(coord));
                let got = n.borders(a)[0].pos as i32;
                assert!(
                    (got - coord as i32).abs() <= 1,
                    "width {width}: asked for {coord}, landed on {got}"
                );
            }
        }
    }

    #[test]
    fn a_drag_is_measured_from_the_split_it_moves() {
        // The inner border's area starts at x=40, not at the window edge.
        let mut n = nested();
        let outer = n.borders(area())[0].clone();
        n.set_ratio(&outer.path, outer.ratio_at(20));
        let inner = n.borders(area())[1].clone();
        assert_eq!(inner.area.x, 20, "it divides what is left of the window");
        assert!(n.set_ratio(&inner.path, inner.ratio_at(6)));
        assert_eq!(n.borders(area())[1].pos, 6);
    }

    #[test]
    fn dragging_the_inner_border_leaves_the_outer_alone() {
        let mut n = nested();
        let before = rect_of(&n, 1, area());
        let inner = n.borders(area())[1].clone();
        n.set_ratio(&inner.path, inner.ratio_at(18));
        assert_eq!(rect_of(&n, 1, area()), before);
        assert_eq!(rect_of(&n, 2, area()).height, 18);
        assert_eq!(rect_of(&n, 3, area()).height, 6);
    }

    #[test]
    fn a_dragged_border_stops_before_crushing_a_pane() {
        let mut n = vsplit();
        for coord in [0, 1, 79, 80, 200] {
            let b = n.borders(area())[0].clone();
            n.set_ratio(&b.path, b.ratio_at(coord));
            let (r1, r2) = (rect_of(&n, 1, area()), rect_of(&n, 2, area()));
            assert!(r1.width >= 1 && r2.width >= 1, "{coord}: {r1:?} {r2:?}");
            assert_eq!(r1.width + r2.width, 80);
        }
    }

    #[test]
    fn a_ratio_is_clamped_however_it_arrives() {
        let mut n = vsplit();
        n.set_ratio(&Path(vec![]), 0);
        assert_eq!(rect_of(&n, 1, area()).width, 8, "the floor is a tenth");
        n.set_ratio(&Path(vec![]), 100);
        assert_eq!(rect_of(&n, 2, area()).width, 8);
    }

    #[test]
    fn a_path_that_leads_nowhere_changes_nothing() {
        let mut n = vsplit();
        let before = n.rects(area());
        assert!(
            !n.set_ratio(&Path(vec![Side::First]), 80),
            "a leaf, not a split"
        );
        assert!(!Node::leaf(1).set_ratio(&Path(vec![]), 80));
        assert_eq!(n.rects(area()), before);
    }

    #[test]
    fn keys_and_drags_move_the_same_border() {
        let mut keys = vsplit();
        keys.resize(1, Dir::Right, RESIZE_STEP);
        let pos = keys.borders(area())[0].pos;

        let mut drag = vsplit();
        let b = drag.borders(area())[0].clone();
        drag.set_ratio(&b.path, b.ratio_at(pos));
        assert_eq!(drag.rects(area()), keys.rects(area()));
    }
}
