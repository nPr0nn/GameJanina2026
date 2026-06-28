//! Small shared editor enums.

/// Identifies one specific classifiable object by its position in the level.
/// Indices are into `Level::sprite_instances` / `Level::collision_shapes`.
/// Using a positional reference (not the string ID) lets two objects that
/// happen to share an ID still be navigated / edited independently.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ObjectRef {
    Sprite(usize),
    CollisionShape(usize),
}

/// The primitive the left mouse button currently places.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tool {
    Rect,
    Circle,
}

/// What a left-drag is currently doing in a shape layer.
#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(clippy::enum_variant_names)] // the shared "Shape" suffix reads clearly here
pub(crate) enum DragAction {
    /// Dragging on empty space to create a brand-new shape.
    NewShape,
    /// Dragging a shape's body to reposition it (the default for a hit).
    MoveShape(usize),
    /// Shift-dragging a shape to redefine its geometry from scratch.
    RedrawShape(usize),
}

#[derive(Clone, Copy, PartialEq)]
#[allow(clippy::enum_variant_names)] // the shared "Planning" suffix is intentional
pub(crate) enum Layer {
    SpritePlanning,
    CollisionPlanning,
    ClassificationPlanning,
}
