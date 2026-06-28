//! Classification-layer labels: spec, text, overlap resolution, outline.

use juni::prelude::*;

use crate::constants::{LABEL_GAP, LABEL_H};
use crate::render::draw_rect_outline;
use crate::types::{EditMode, ObjectRef};

/// A deferred label for the classification layer. All labels are collected,
/// overlap-resolved, then drawn in a single second pass so no two labels
/// whose x-ranges overlap are rendered on top of each other.
pub(crate) struct LabelSpec {
    pub(crate) x: f32,
    /// Resolved y (top of label rect). Starts as the preferred position
    /// and is pushed downward by [`resolve_label_overlaps`] as needed.
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) text: String,
    pub(crate) text_color: Color,
    pub(crate) bg_color: Color,
}

/// Draw the bounding-box outline for one classifiable object.
/// Labels are NOT drawn here — they are collected into [`LabelSpec`]s,
/// overlap-resolved by [`resolve_label_overlaps`], and rendered afterwards.
pub(crate) fn draw_classification_outline(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tag_color: Color,
    is_focused: bool,
    is_editing: bool,
) {
    let (thickness, box_color) = if is_editing {
        (3.0, WHITE)
    } else if is_focused {
        (2.5, GOLD)
    } else {
        (1.5, tag_color.with_alpha(0.9))
    };
    draw_rect_outline(canvas, x, y, w, h, thickness, box_color);
}

/// Build the label string for a classifiable object.
///
/// Format:
/// - Normal:      `"id | tag"`
/// - Editing tag: `"id | buffer|"`
/// - Editing ID:  `"buffer| | tag"`
pub(crate) fn build_label_text(
    id: &str,
    tag: &str,
    edit_state: Option<&(ObjectRef, EditMode, String)>,
) -> String {
    if let Some((_, mode, buf)) = edit_state {
        match mode {
            EditMode::Tag => format!("{id} | {buf}|"),
            EditMode::ObjectId => format!("{buf}| | {tag}"),
        }
    } else {
        format!("{id} | {tag}")
    }
}

/// Push [`LabelSpec`]s apart so that no two labels whose x-ranges overlap
/// are rendered on top of each other.
///
/// Algorithm: sort by preferred y, then do an O(n²) forward pass — each
/// label can push every subsequent label downward if they share an x-range
/// and are too close vertically. Because pushed labels are only ever moved
/// downward, and j > i in the inner loop sees the already-updated y of i,
/// cascades propagate correctly in a single pass.
pub(crate) fn resolve_label_overlaps(labels: &mut Vec<LabelSpec>) {
    labels.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    let n = labels.len();
    for i in 0..n {
        for j in (i + 1)..n {
            // Only push if the two labels share a horizontal band.
            let x_overlap =
                labels[j].x < labels[i].x + labels[i].w && labels[j].x + labels[j].w > labels[i].x;
            if x_overlap {
                let needed = labels[i].y + LABEL_H + LABEL_GAP;
                if labels[j].y < needed {
                    labels[j].y = needed;
                }
            }
        }
    }
}
