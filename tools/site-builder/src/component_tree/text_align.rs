use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) enum TextAlign {
    Start,
    Center,
    End,
}
