/// The structure box a piece's `bounding_box` answers, in world blocks.
pub use sunrise_core::bounding_box::BoundingBox;
pub use sunrise_core::equipment_slot::EquipmentSlot;
/// An owned, validated resource name, not a process-local registry key.
///
/// In Wire, including typed payloads, the namespace and path are written in that order.
/// Each is a UTF-8 string with a little-endian `u32` byte length. Typed payloads apply
/// their usual string framing and allocation budget; decoding does not resolve or intern a key.
pub use sunrise_core::identifier::Identifier;
/// A level's key (`LevelDimension::Custom`).
pub use sunrise_core::level_key::LevelKey;
pub use sunrise_core::nbt::Compound;
/// The NBT string: every `Compound` key and `Tag::String`, held as UTF-16 so it can keep Java's unpaired surrogates.
pub use sunrise_core::nbt::JavaString;
pub use sunrise_core::nbt::Tag;
pub use sunrise_core::use_animation::UseAnimation;
pub use {
    sunrise_core::{
        connection_kind::ConnectionKind, dimension::Dimension as LevelDimension,
        neighbour_update::NeighbourUpdate,
    },
    sunrise_mod_runtime::attachments::{
        AttachmentCodec, AttachmentError, AttachmentForm, AttachmentPolicy, AttachmentStore,
        AttachmentType, AttachmentValue, AttachmentWrite, Holder,
    },
};
// hidden: the game's maps behind the store, not part of the mod surface
#[doc(hidden)]
pub use sunrise_mod_runtime::attachments::{AttachmentAccess, AttachmentHost, Attachments};
