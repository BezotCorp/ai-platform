use crate::sqlite::{readers::Readers, writer::Writer};

pub(crate) struct DatabaseInner {
    pub(crate) writer: Writer,
    pub(crate) readers: Readers,
}
