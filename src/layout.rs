use ratatui::layout::Rect;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Split {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
}

pub fn grid(count: usize, split: Split) -> Grid {
    let n = count.max(1) as u16;
    match (n, split) {
        (4, _) => Grid { cols: 2, rows: 2 },
        (n, Split::Vertical) => Grid { cols: n, rows: 1 },
        (n, Split::Horizontal) => Grid { cols: 1, rows: n },
    }
}

/// Divides `total` into `parts` slices, handing the remainder to the leading
/// slices so the pieces always add back up to `total`.
fn slices(origin: u16, total: u16, parts: u16) -> Vec<(u16, u16)> {
    let base = total / parts;
    let extra = total % parts;
    let mut out = Vec::with_capacity(parts as usize);
    let mut at = origin;
    for i in 0..parts {
        let len = base + if i < extra { 1 } else { 0 };
        out.push((at, len));
        at += len;
    }
    out
}

pub fn areas(area: Rect, count: usize, split: Split) -> Vec<Rect> {
    let g = grid(count, split);
    let xs = slices(area.x, area.width, g.cols);
    let ys = slices(area.y, area.height, g.rows);
    let mut out = Vec::with_capacity(count);
    'outer: for (y, h) in &ys {
        for (x, w) in &xs {
            if out.len() == count {
                break 'outer;
            }
            out.push(Rect::new(*x, *y, *w, *h));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_fills_the_grid() {
        assert_eq!(grid(1, Split::Vertical), Grid { cols: 1, rows: 1 });
        assert_eq!(grid(1, Split::Horizontal), Grid { cols: 1, rows: 1 });
    }

    #[test]
    fn vertical_split_lays_panes_side_by_side() {
        assert_eq!(grid(2, Split::Vertical), Grid { cols: 2, rows: 1 });
        assert_eq!(grid(3, Split::Vertical), Grid { cols: 3, rows: 1 });
    }

    #[test]
    fn horizontal_split_stacks_panes() {
        assert_eq!(grid(2, Split::Horizontal), Grid { cols: 1, rows: 2 });
        assert_eq!(grid(3, Split::Horizontal), Grid { cols: 1, rows: 3 });
    }

    #[test]
    fn four_panes_always_form_a_quad() {
        assert_eq!(grid(4, Split::Vertical), Grid { cols: 2, rows: 2 });
        assert_eq!(grid(4, Split::Horizontal), Grid { cols: 2, rows: 2 });
    }

    #[test]
    fn areas_split_the_rect_side_by_side() {
        let a = areas(Rect::new(0, 0, 80, 24), 2, Split::Vertical);
        assert_eq!(a, vec![Rect::new(0, 0, 40, 24), Rect::new(40, 0, 40, 24)]);
    }

    #[test]
    fn areas_split_the_rect_into_stacked_rows() {
        let a = areas(Rect::new(0, 0, 80, 24), 2, Split::Horizontal);
        assert_eq!(a, vec![Rect::new(0, 0, 80, 12), Rect::new(0, 12, 80, 12)]);
    }

    #[test]
    fn areas_for_four_panes_are_quadrants() {
        let a = areas(Rect::new(0, 0, 80, 24), 4, Split::Vertical);
        assert_eq!(
            a,
            vec![
                Rect::new(0, 0, 40, 12),
                Rect::new(40, 0, 40, 12),
                Rect::new(0, 12, 40, 12),
                Rect::new(40, 12, 40, 12),
            ]
        );
    }

    #[test]
    fn areas_absorb_remainders_so_no_cell_is_lost() {
        let a = areas(Rect::new(0, 0, 81, 25), 2, Split::Vertical);
        assert_eq!(a[0].width + a[1].width, 81);
        assert_eq!(a[1].x, a[0].x + a[0].width);
    }

    #[test]
    fn areas_are_offset_by_the_parent_origin() {
        let a = areas(Rect::new(10, 5, 80, 24), 2, Split::Vertical);
        assert_eq!(a[0], Rect::new(10, 5, 40, 24));
        assert_eq!(a[1], Rect::new(50, 5, 40, 24));
    }

    #[test]
    fn three_stacked_panes_divide_the_height() {
        let a = areas(Rect::new(0, 0, 80, 30), 3, Split::Horizontal);
        assert_eq!(a.len(), 3);
        assert_eq!(a.iter().map(|r| r.height).sum::<u16>(), 30);
        assert_eq!(a[2].y, 20);
    }
}
