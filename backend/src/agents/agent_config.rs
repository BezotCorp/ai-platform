use serde::{Deserialize, Deserializer, de::Error};
use serde_json::{Map, Value};

use crate::{
    agents::{AgentIdentity, AgentRole},
    providers::Model,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentConfig {
    pub identity: AgentIdentity,
    pub role: AgentRole,
    pub model: Model,
}

impl AgentConfig {
    fn take_string<E: Error>(fields: &mut Map<String, Value>, name: &str) -> Result<String, E> {
        match fields.remove(name) {
            Some(Value::String(value)) => Ok(value),
            _ => Err(E::custom(format!(
                "Champ d'agent manquant ou invalide : {name}"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for AgentConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
        let id = Self::take_string::<D::Error>(&mut fields, "id")?;
        let role = Self::take_string::<D::Error>(&mut fields, "role")?;
        let instructions = Self::take_string::<D::Error>(&mut fields, "instructions")?;
        let provider = Self::take_string::<D::Error>(&mut fields, "provider")?;
        let model = Self::take_string::<D::Error>(&mut fields, "model")?;
        if !fields.is_empty() {
            return Err(D::Error::custom("Champs d'agent inconnus"));
        }
        if provider != "ollama" {
            return Err(D::Error::custom("Fournisseur non connecté"));
        }
        if instructions.len() > 8192 {
            return Err(D::Error::custom("Instructions trop volumineuses"));
        }
        Ok(Self {
            identity: AgentIdentity::new(id).map_err(D::Error::custom)?,
            role: AgentRole::new(role, instructions).map_err(D::Error::custom)?,
            model: Model::new(provider, model).map_err(D::Error::custom)?,
        })
    }
}
