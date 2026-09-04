use crate::TypeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    Owned,
    Moved,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub ty: TypeId,
    pub ownership: Ownership,
}
