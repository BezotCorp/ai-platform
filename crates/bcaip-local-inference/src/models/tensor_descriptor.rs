use crate::models::tensor_data_type::TensorDataType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorDescriptor {
    pub name: String,
    pub shape: Vec<u64>,
    pub data_type: TensorDataType,
}
