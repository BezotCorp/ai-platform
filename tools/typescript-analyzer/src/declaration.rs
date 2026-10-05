use crate::declaration_kind::DeclarationKind;

#[derive(Debug)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub name: String,
    pub line: usize,
}

impl Declaration {
    pub fn new(kind: DeclarationKind, name: impl Into<String>, line: usize) -> Self {
        Self {
            kind,
            name: name.into(),
            line,
        }
    }
}
