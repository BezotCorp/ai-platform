use std::any::Any;

pub(crate) type OperationResult = Box<dyn Any + Send>;
