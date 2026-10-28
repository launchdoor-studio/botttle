//! The pane tree.
//!
//! A tab holds one [`PaneGroup`]: either a single pane or nested rows and columns
//! of them. Splitting along the axis a pane already lives on adds a sibling
//! instead of nesting deeper, which is what keeps repeated splits flat and
//! predictable.
//!
//! The gutter between two neighbours is a handle: dragging it moves space from
//! one to the other, along whichever axis the two share.

use std::rc::Rc;

use gpui::{
    div, px, AnyElement, App, AppContext, Axis, CursorStyle, Entity, EntityId, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled,
    Window,
};

use crate::terminal::view::TerminalView;
use crate::theme::Theme;

/// Gap between neighbouring panes, in pixels. It doubles as the resize handle.
const GUTTER: f32 = 6.0;
/// A pane can't be dragged smaller than this along the axis being resized.
const MIN_PANE_SIZE: f32 = 80.0;

/// A request to move the divider after member `divider` of the axis at `path`
/// so that it sits `position` pixels into an axis `extent` pixels long.
pub struct Resize {
    path: Vec<usize>,
    divider: usize,
    position: f32,
    extent: f32,
}

/// What a resize handle carries while it is dragged.
struct DraggedDivider {
    path: Vec<usize>,
    divider: usize,
}

/// Drawn under the pointer during a drag. The handle itself is the feedback,
/// so this draws nothing.
struct DividerDragPreview;

impl Render for DividerDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub type ResizeHandler = Rc<dyn Fn(&Resize, &mut Window, &mut App)>;

pub struct PaneGroup {
    root: Member,
}

enum Member {
    Pane(Entity<TerminalView>),
    Axis(PaneAxis),
}

struct PaneAxis {
    axis: Axis,
    members: Vec<Member>,
    /// Each member's share of the axis, as flex-grow weights. Only the ratios
    /// matter; a fresh split gives everyone the same weight.
    flexes: Vec<f32>,
}

/// What happened when a pane was closed.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveResult {
    /// The pane is gone and the group still has panes left.
    Removed,
    /// The pane was the last one, so the whole group should go away.
    GroupEmpty,
    NotFound,
}

impl PaneGroup {
    pub fn new(pane: Entity<TerminalView>) -> Self {
        Self {
            root: Member::Pane(pane),
        }
    }

    /// Splits `target` along `axis`, placing `new_pane` after it.
    pub fn split(&mut self, target: EntityId, axis: Axis, new_pane: Entity<TerminalView>) -> bool {
        self.root.split(target, axis, new_pane)
    }

    pub fn remove(&mut self, target: EntityId) -> RemoveResult {
        match &self.root {
            Member::Pane(pane) if pane.entity_id() == target => RemoveResult::GroupEmpty,
            Member::Pane(_) => RemoveResult::NotFound,
            Member::Axis(_) => {
                if self.root.remove(target) {
                    RemoveResult::Removed
                } else {
                    RemoveResult::NotFound
                }
            }
        }
    }

    /// Every pane, left to right and top to bottom. Used for cycling focus.
    pub fn panes(&self) -> Vec<Entity<TerminalView>> {
        let mut panes = Vec::new();
        self.root.collect(&mut panes);
        panes
    }

    /// Moves a divider in response to a drag.
    pub fn resize(&mut self, resize: &Resize) {
        let mut member = &mut self.root;
        for &index in &resize.path {
            let Member::Axis(pane_axis) = member else {
                return;
            };
            let Some(child) = pane_axis.members.get_mut(index) else {
                return;
            };
            member = child;
        }
        if let Member::Axis(pane_axis) = member {
            move_divider(
                &mut pane_axis.flexes,
                resize.divider,
                resize.position,
                resize.extent,
            );
        }
    }

    pub fn render(
        &self,
        active: Option<EntityId>,
        theme: &Theme,
        on_resize: &ResizeHandler,
    ) -> AnyElement {
        self.root.render(active, theme, &mut Vec::new(), on_resize)
    }
}

impl Member {
    fn split(&mut self, target: EntityId, axis: Axis, new_pane: Entity<TerminalView>) -> bool {
        match self {
            Member::Pane(pane) => {
                if pane.entity_id() != target {
                    return false;
                }
                let existing = Member::Pane(pane.clone());
                *self = Member::Axis(PaneAxis {
                    axis,
                    members: vec![existing, Member::Pane(new_pane)],
                    flexes: vec![1.0, 1.0],
                });
                true
            }
            Member::Axis(pane_axis) => {
                if pane_axis.axis == axis {
                    let position = pane_axis.members.iter().position(
                        |member| matches!(member, Member::Pane(pane) if pane.entity_id() == target),
                    );
                    if let Some(index) = position {
                        // The new pane takes half of the one it split, so the
                        // rest of the row keeps the sizes it was dragged to.
                        let half = pane_axis.flexes[index] / 2.0;
                        pane_axis.flexes[index] = half;
                        pane_axis.flexes.insert(index + 1, half);
                        pane_axis.members.insert(index + 1, Member::Pane(new_pane));
                        return true;
                    }
                }

                pane_axis
                    .members
                    .iter_mut()
                    .any(|member| member.split(target, axis, new_pane.clone()))
            }
        }
    }

    fn remove(&mut self, target: EntityId) -> bool {
        let Member::Axis(pane_axis) = self else {
            return false;
        };

        let position = pane_axis
            .members
            .iter()
            .position(|member| matches!(member, Member::Pane(pane) if pane.entity_id() == target));

        match position {
            Some(index) => {
                pane_axis.members.remove(index);
                // The space goes to a neighbour rather than being shared out,
                // so panes elsewhere in the row stay where they were.
                let freed = pane_axis.flexes.remove(index);
                let neighbour = index.saturating_sub(1).min(pane_axis.flexes.len() - 1);
                pane_axis.flexes[neighbour] += freed;
            }
            None => {
                if !pane_axis
                    .members
                    .iter_mut()
                    .any(|member| member.remove(target))
                {
                    return false;
                }
            }
        }

        // An axis with a single child is just that child.
        if pane_axis.members.len() == 1 {
            let only = pane_axis.members.pop().expect("checked length");
            *self = only;
        }
        true
    }

    fn collect(&self, panes: &mut Vec<Entity<TerminalView>>) {
        match self {
            Member::Pane(pane) => panes.push(pane.clone()),
            Member::Axis(pane_axis) => {
                for member in &pane_axis.members {
                    member.collect(panes);
                }
            }
        }
    }

    fn render(
        &self,
        active: Option<EntityId>,
        theme: &Theme,
        path: &mut Vec<usize>,
        on_resize: &ResizeHandler,
    ) -> AnyElement {
        match self {
            Member::Pane(pane) => {
                let is_focused = active == Some(pane.entity_id());
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded(Theme::radius())
                    .border_1()
                    // The focused pane is marked by its border alone: enough to
                    // find at a glance, quiet enough to ignore while reading output.
                    .border_color(if is_focused {
                        theme.focus_ring()
                    } else {
                        theme.border
                    })
                    .child(pane.clone())
                    .into_any_element()
            }
            Member::Axis(pane_axis) => {
                let axis = pane_axis.axis;
                let mut children = Vec::with_capacity(pane_axis.members.len() * 2);
                for (index, member) in pane_axis.members.iter().enumerate() {
                    if index > 0 {
                        children.push(render_handle(axis, path, index - 1, theme));
                    }
                    path.push(index);
                    let mut cell = div()
                        .flex()
                        .flex_basis(px(0.0))
                        .min_w_0()
                        .min_h_0()
                        .child(member.render(active, theme, path, on_resize));
                    cell.style().flex_grow = Some(pane_axis.flexes[index]);
                    children.push(cell.into_any_element());
                    path.pop();
                }

                let own_path = path.clone();
                let on_resize = on_resize.clone();
                let container = div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    // Every axis hears every divider drag; each acts only on its
                    // own, and measures against its own bounds.
                    .on_drag_move(
                        move |event: &gpui::DragMoveEvent<DraggedDivider>, window, cx| {
                            let drag = event.drag(cx);
                            if drag.path != own_path {
                                return;
                            }
                            let (position, origin, extent) = match axis {
                                Axis::Horizontal => (
                                    event.event.position.x,
                                    event.bounds.origin.x,
                                    event.bounds.size.width,
                                ),
                                Axis::Vertical => (
                                    event.event.position.y,
                                    event.bounds.origin.y,
                                    event.bounds.size.height,
                                ),
                            };
                            let resize = Resize {
                                path: drag.path.clone(),
                                divider: drag.divider,
                                position: f32::from(position - origin),
                                extent: f32::from(extent),
                            };
                            on_resize(&resize, window, cx);
                        },
                    );
                let container = match axis {
                    Axis::Horizontal => container.flex_row(),
                    Axis::Vertical => container.flex_col(),
                };
                container.children(children).into_any_element()
            }
        }
    }
}

/// The gutter after member `divider`, which can be dragged to resize the two
/// members either side of it.
fn render_handle(axis: Axis, path: &[usize], divider: usize, theme: &Theme) -> AnyElement {
    let id = SharedString::from(format!("divider-{path:?}-{divider}"));
    let gutter: Pixels = px(GUTTER);
    let handle = div().id(id).flex_none().rounded(px(2.0));
    let handle = match axis {
        Axis::Horizontal => handle
            .w(gutter)
            .h_full()
            .cursor(CursorStyle::ResizeLeftRight),
        Axis::Vertical => handle.h(gutter).w_full().cursor(CursorStyle::ResizeUpDown),
    };
    handle
        .hover(|style| style.bg(theme.border))
        .on_drag(
            DraggedDivider {
                path: path.to_vec(),
                divider,
            },
            |_, _, _, cx| cx.new(|_| DividerDragPreview),
        )
        .into_any_element()
}

/// Moves the boundary between members `divider` and `divider + 1` to
/// `position` along an axis `extent` long, trading space only between those
/// two and never shrinking either below [`MIN_PANE_SIZE`].
fn move_divider(flexes: &mut [f32], divider: usize, position: f32, extent: f32) {
    let count = flexes.len();
    if divider + 1 >= count {
        return;
    }
    let total: f32 = flexes.iter().sum();
    let available = extent - GUTTER * (count - 1) as f32;
    if total <= 0.0 || available <= 0.0 {
        return;
    }

    let size = |flex: f32| flex / total * available;
    let start: f32 = flexes[..divider]
        .iter()
        .map(|&flex| size(flex))
        .sum::<f32>()
        + GUTTER * divider as f32;
    let combined = size(flexes[divider]) + size(flexes[divider + 1]);
    if combined < MIN_PANE_SIZE * 2.0 {
        return;
    }

    // The pointer holds the middle of the gutter.
    let first = (position - start - GUTTER / 2.0).clamp(MIN_PANE_SIZE, combined - MIN_PANE_SIZE);
    flexes[divider] = first / available * total;
    flexes[divider + 1] = (combined - first) / available * total;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(flexes: &[f32], extent: f32) -> Vec<f32> {
        let total: f32 = flexes.iter().sum();
        let available = extent - GUTTER * (flexes.len() - 1) as f32;
        flexes.iter().map(|flex| flex / total * available).collect()
    }

    #[test]
    fn dragging_moves_the_boundary_to_the_pointer() {
        let mut flexes = vec![1.0, 1.0];
        let extent = 1006.0;
        move_divider(&mut flexes, 0, 303.0, extent);
        let sizes = sizes(&flexes, extent);
        assert!((sizes[0] - 300.0).abs() < 0.01, "{sizes:?}");
        assert!((sizes[1] - 700.0).abs() < 0.01, "{sizes:?}");
    }

    #[test]
    fn only_the_two_neighbours_change() {
        let mut flexes = vec![1.0, 1.0, 1.0];
        let extent = 912.0;
        let before = sizes(&flexes, extent);
        move_divider(&mut flexes, 1, 500.0, extent);
        let after = sizes(&flexes, extent);
        assert!((after[0] - before[0]).abs() < 0.01);
        assert!((after[1] + after[2] - before[1] - before[2]).abs() < 0.01);
    }

    #[test]
    fn panes_keep_a_minimum_size() {
        let mut flexes = vec![1.0, 1.0];
        let extent = 1006.0;
        move_divider(&mut flexes, 0, -500.0, extent);
        assert!((sizes(&flexes, extent)[0] - MIN_PANE_SIZE).abs() < 0.01);
        move_divider(&mut flexes, 0, 5000.0, extent);
        assert!((sizes(&flexes, extent)[1] - MIN_PANE_SIZE).abs() < 0.01);
    }

    #[test]
    fn a_divider_past_the_end_is_ignored() {
        let mut flexes = vec![1.0, 1.0];
        move_divider(&mut flexes, 1, 100.0, 1000.0);
        assert_eq!(flexes, vec![1.0, 1.0]);
    }
}
