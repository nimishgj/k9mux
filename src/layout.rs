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
            Node::Split { dir, first, second } => {
                match (first.remove_leaf(target), second.remove_leaf(target)) {
                    (None, None) => None,
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (Some(a), Some(b)) => Some(Node::Split {
                        dir,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                }
            }
        }
    }

    /// Where every pane sits inside `area`.
    pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            Node::Leaf(id) => vec![(*id, area)],
            Node::Split { dir, first, second } => {
                let (a, b) = halve(area, *dir);
                let mut out = first.rects(a);
                out.extend(second.rects(b));
                out
            }
        }
    }
}

/// Divides a rect in two, giving the remainder to the first half.
fn halve(area: Rect, dir: Split) -> (Rect, Rect) {
    match dir {
        Split::Vertical => {
            let left = area.width.div_ceil(2);
            (
                Rect::new(area.x, area.y, left, area.height),
                Rect::new(area.x + left, area.y, area.width - left, area.height),
            )
        }
        Split::Horizontal => {
            let top = area.height.div_ceil(2);
            (
                Rect::new(area.x, area.y, area.width, top),
                Rect::new(area.x, area.y + top, area.width, area.height - top),
            )
        }
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
}
