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

/// Which field is currently being edited on the focused object.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum EditMode {
    Tag,
    ObjectId,
}

/// The primitive the left mouse button currently places.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tool {
    Rect,
    Circle,
}

/// What a left-drag is currently doing in a shape layer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum DragAction {
    /// Dragging on empty space to create a brand-new shape.
    NewShape,
    /// Dragging to redefine the geometry of an already-selected shape.
    RedrawShape(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Layer {
    SpritePlanning,
    CollisionPlanning,
    ClassificationPlanning,
}
