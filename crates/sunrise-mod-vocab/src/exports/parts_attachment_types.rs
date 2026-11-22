//! Engine attachment declaration validation.
// hidden: the engine's registry lookup; authors bind typed keys and snapshots.
#[doc(hidden)]
pub use crate::parts::AttachmentDeclarations;
mod __private {
    pub use crate::parts::{
        AttachmentTypeDeclaration, validate_attachment_use, validate_declared_attachment,
    };
}
