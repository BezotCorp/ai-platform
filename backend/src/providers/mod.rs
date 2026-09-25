mod model;
mod ollama;
mod provider;

pub(crate) use model::Model;
pub(crate) use ollama::{AvailableModel, Chat, ChatTurn, Client};
