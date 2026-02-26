use crate::error::CoreError;

pub type CoreResult<T> = Result<T, CoreError>;
