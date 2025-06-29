use crate::*;
use slab::Slab;

pub struct Text {
    pub(crate) text_boxes: Slab<TextBox<String>>,
    pub(crate) static_text_boxes: Slab<TextBox<&'static str>>,
    pub(crate) text_edits: Slab<TextEdit>,
    pub(crate) styles: Slab<TextStyle2>,
}

pub struct TextBoxHandle {
    pub(crate) i: u32,
    pub(crate) kind: TextBoxKind,
}

pub(crate) enum TextBoxKind {
    StringBox,
    StaticBox,
    Edit,
}
