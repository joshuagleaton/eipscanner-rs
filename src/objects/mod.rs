//! Standard CIP objects: Identity, Parameter, and File.

pub mod file;
mod identity;
mod parameter;

pub use file::{FileObject, FileObjectStateCode};
pub use identity::IdentityObject;
pub use parameter::{CipValue, ParameterObject};
