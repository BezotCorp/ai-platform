use crate::models::{ModelFormat, TensorDescriptor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub format: ModelFormat,
    pub tensors: Vec<TensorDescriptor>,
}
