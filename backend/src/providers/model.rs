#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Model {
    pub provider: String,
    pub name: String,
}

impl Model {
    pub fn new(
        provider: String,
        name: String,
    ) -> Result<Self, &'static str> {
        if provider.trim().is_empty() {
            return Err("Model provider cannot be empty");
        }

        if name.trim().is_empty() {
            return Err("Model name cannot be empty");
        }

        Ok(Self { provider, name })
    }
}
