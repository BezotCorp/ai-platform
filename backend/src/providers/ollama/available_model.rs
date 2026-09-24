use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct AvailableModel {
    pub name: String,
}
