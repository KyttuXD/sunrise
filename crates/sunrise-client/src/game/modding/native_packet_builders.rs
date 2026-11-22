//! The client host encodes typed play packets and hashed container clicks.
use super::Game;
use mod_api::{ContainerClickRequest, PacketBuildError, PlayPacketRequest, ServerboundPlayPacket};

impl Game<'_> {
    pub(super) fn build_play_packet(
        request: &PlayPacketRequest,
    ) -> Result<ServerboundPlayPacket, PacketBuildError> {
        sunrise_client::mod_packet_builders::encode_play_packet(request)
    }

    pub(super) fn container_click(
        &self,
        request: &ContainerClickRequest,
    ) -> Result<ServerboundPlayPacket, PacketBuildError> {
        ServerboundPlayPacket::validate_container_click_limits(
            request.container_id,
            request.state_id,
            request.slot,
            request.changed_slots.len() as u64,
        )?;
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| PacketBuildError::Unavailable {
                packet: "container_click".into(),
                field: "connection".into(),
            })?;
        sunrise_client::mod_packet_builders::encode_container_click(
            request,
            &remote.builtin,
            &remote.registries,
            |stack| super::api_values::stack_value(stack).ok(),
        )
    }
}

// These hosts can encode stateless requests even when Game or its world is not available.
macro_rules! impl_packet_builders_without_connection {
    () => {
        fn build_play_packet(
            &self,
            request: &mod_api::PlayPacketRequest,
        ) -> Result<mod_api::ServerboundPlayPacket, mod_api::PacketBuildError> {
            use mod_api::__private::{LoadAccess, access_allowed};
            if !access_allowed("build_play_packet", LoadAccess::Metadata) {
                return Err(mod_api::AccessRefusal::unavailable());
            }
            sunrise_client::mod_packet_builders::encode_play_packet(request)
        }

        fn container_click(
            &self,
            _: &mod_api::ContainerClickRequest,
        ) -> Result<mod_api::ServerboundPlayPacket, mod_api::PacketBuildError> {
            Err(mod_api::NoBackend::no_backend())
        }
    };
}
pub(super) use impl_packet_builders_without_connection;

#[cfg(test)]
#[path = "native_packet_builders/tests.rs"]
mod tests;
