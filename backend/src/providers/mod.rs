mod model;
mod ollama;

pub(crate) use model::Model;
pub(crate) use ollama::{AvailableModel, Chat, ChatTurn, Client};
