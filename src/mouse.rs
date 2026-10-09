//! What a mouse event does. Clicks pick things; a press on a pane border
//! grabs it and drags it, the way tmux does.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::App;
use crate::ui;

/// Applies one mouse event. `area` is the whole frame, the same one the last
/// draw used, so what the pointer is over is what is on screen.
pub fn handle(app: &mut App, area: Rect, m: MouseEvent) {
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // A border is checked for first: a press on one is the start of
            // a drag, not a click on either of the panes it divides.
            if let Some(path) = ui::border_hit(app, area, m.column, m.row) {
                app.drag = Some(path);
            } else {
                pick(app, area, m.column, m.row);
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(path) = app.drag.clone()
                && let Some(ratio) = ui::border_ratio(app, area, &path, m.column, m.row)
            {
                app.drag_border(&path, ratio);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => app.drag = None,
        _ => {}
    }
}

/// Focuses whatever was clicked: a sidebar row, or a pane.
fn pick(app: &mut App, area: Rect, x: u16, y: u16) {
    match ui::sidebar_hit(app, x, y) {
        Some(ui::Row::Window(w)) => app.select_window(w),
        Some(ui::Row::Pane { window, pane }) => {
            app.select_window(window);
            app.set_focus(pane);
        }
        None => {
            if let Some(idx) = ui::pane_hit(app, area, x, y) {
                app.set_focus(idx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Spawner;
    use crate::fleet::{Fleet, Grouping};
    use crate::keys::Action;
    use crate::layout::Split;
    use crate::session::Launch;
    use crate::ui::SIDEBAR_WIDTH;

    fn stub() -> Spawner {
        Box::new(|_: &str| Launch::new("sh").arg("-c").arg("sleep 30"))
    }

    fn fleet() -> Fleet {
        Fleet::from_contexts(
            &["use1-a".to_string(), "use1-b".to_string()],
            &Grouping {
                clusters: vec![("prod".into(), vec!["use1-a".into(), "use1-b".into()])],
            },
        )
    }

    const AREA: Rect = Rect::new(0, 0, 140, 40);
    /// Where two side-by-side panes meet in a 140-column frame.
    const BORDER_X: u16 = SIDEBAR_WIDTH + 55;

    fn split_app() -> App {
        let mut a = App::new(fleet(), stub());
        a.apply(Action::Split(Split::Vertical));
        a
    }

    fn at(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    fn press(a: &mut App, x: u16, y: u16) {
        handle(a, AREA, at(MouseEventKind::Down(MouseButton::Left), x, y));
    }
    fn drag(a: &mut App, x: u16, y: u16) {
        handle(a, AREA, at(MouseEventKind::Drag(MouseButton::Left), x, y));
    }
    fn release(a: &mut App, x: u16, y: u16) {
        handle(a, AREA, at(MouseEventKind::Up(MouseButton::Left), x, y));
    }

    fn widths(a: &App) -> Vec<u16> {
        ui::pane_areas(a, AREA)
            .into_iter()
            .map(|(_, r)| r.width)
            .collect()
    }

    #[test]
    fn dragging_a_border_resizes_the_panes_either_side() {
        let mut a = split_app();
        assert_eq!(widths(&a), vec![55, 55]);

        press(&mut a, BORDER_X, 10);
        drag(&mut a, SIDEBAR_WIDTH + 30, 10);
        release(&mut a, SIDEBAR_WIDTH + 30, 10);

        assert_eq!(widths(&a), vec![30, 80]);
    }

    #[test]
    fn a_drag_follows_the_pointer_the_whole_way() {
        let mut a = split_app();
        press(&mut a, BORDER_X, 10);
        for x in [100, 90, 70, 50, 45, 60, 120] {
            drag(&mut a, x, 10);
            let pos = ui::borders(&a, AREA)[0].pos as i32;
            assert!((pos - x as i32).abs() <= 1, "pointer {x}, border {pos}");
        }
        release(&mut a, 120, 10);
        assert_eq!(a.drag, None, "and then it lets go");
    }

    #[test]
    fn a_drag_can_be_taken_back_the_way_it_came() {
        let mut a = split_app();
        let before = widths(&a);
        press(&mut a, BORDER_X, 10);
        drag(&mut a, 50, 10);
        assert_ne!(widths(&a), before);
        drag(&mut a, BORDER_X, 10);
        release(&mut a, BORDER_X, 10);
        assert_eq!(widths(&a), before, "back to an even split");
    }

    #[test]
    fn a_stacked_pair_is_dragged_up_and_down() {
        let mut a = App::new(fleet(), stub());
        a.apply(Action::Split(Split::Horizontal));
        let border_y = ui::borders(&a, AREA)[0].pos;

        press(&mut a, 80, border_y);
        drag(&mut a, 80, 30);
        release(&mut a, 80, 30);

        let heights: Vec<u16> = ui::pane_areas(&a, AREA)
            .into_iter()
            .map(|(_, r)| r.height)
            .collect();
        assert_eq!(heights, vec![30, 10]);
    }

    #[test]
    fn a_move_with_nothing_grabbed_changes_nothing() {
        let mut a = split_app();
        let before = widths(&a);
        drag(&mut a, 50, 10);
        assert_eq!(widths(&a), before);
        assert_eq!(a.drag, None);
    }

    #[test]
    fn letting_go_ends_the_drag() {
        let mut a = split_app();
        press(&mut a, BORDER_X, 10);
        drag(&mut a, 50, 10);
        release(&mut a, 50, 10);
        let after = widths(&a);

        // The pointer wanders on with the button up.
        drag(&mut a, 120, 10);
        assert_eq!(widths(&a), after, "the border stayed where it was dropped");
    }

    #[test]
    fn a_press_inside_a_pane_focuses_it_without_grabbing_anything() {
        let mut a = split_app();
        let before = widths(&a);
        press(&mut a, BORDER_X + 20, 10);
        assert_eq!(a.focus(), 1, "the pane that was clicked");
        assert_eq!(a.drag, None);

        drag(&mut a, 50, 10);
        assert_eq!(
            widths(&a),
            before,
            "a drag from inside a pane resizes nothing"
        );
    }

    #[test]
    fn a_press_in_the_sidebar_still_selects_a_pane() {
        let mut a = split_app();
        let rows = ui::sidebar_rows(&a);
        let (y, _) = rows
            .iter()
            .find(|(_, r)| matches!(r, ui::Row::Pane { pane: 1, .. }))
            .copied()
            .unwrap();
        press(&mut a, 4, y);
        assert_eq!(a.focus(), 1);
        assert_eq!(a.drag, None);
    }

    #[test]
    fn a_press_in_the_sidebar_still_selects_a_window() {
        let mut a = split_app();
        a.apply(Action::NewWindow);
        assert_eq!(a.current, 1);
        let y = ui::sidebar_rows(&a)
            .into_iter()
            .find(|(_, r)| *r == ui::Row::Window(0))
            .unwrap()
            .0;
        press(&mut a, 4, y);
        assert_eq!(a.current, 0);
    }

    #[test]
    fn a_zoomed_pane_has_no_border_to_drag() {
        let mut a = split_app();
        a.apply(Action::ToggleZoom);
        press(&mut a, BORDER_X, 10);
        drag(&mut a, 50, 10);
        assert_eq!(a.drag, None);
        assert_eq!(widths(&a), vec![AREA.width - SIDEBAR_WIDTH]);
    }

    #[test]
    fn a_drag_survives_a_window_that_grew_under_it() {
        // The frame can change size mid-drag; the border is addressed by the
        // split it belongs to, not by where it happened to be.
        let mut a = split_app();
        press(&mut a, BORDER_X, 10);
        let wide = Rect::new(0, 0, 200, 40);
        handle(
            &mut a,
            wide,
            at(MouseEventKind::Drag(MouseButton::Left), 100, 10),
        );
        let pos = ui::borders(&a, wide)[0].pos as i32;
        assert!((pos - 100).abs() <= 1, "border at {pos}");
    }

    #[test]
    fn other_buttons_are_left_alone() {
        let mut a = split_app();
        let before = widths(&a);
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Drag(MouseButton::Right),
            MouseEventKind::Moved,
            MouseEventKind::ScrollDown,
        ] {
            handle(&mut a, AREA, at(kind, BORDER_X, 10));
        }
        assert_eq!(a.drag, None);
        assert_eq!(widths(&a), before);
    }
}
