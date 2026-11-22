# Sending vanilla play packets

`ClientHost::send_packet` submits an owned `ServerboundPlayPacket`. The game validates the
body once and reserves outgoing credit. Publication waits for the current root transaction to
commit; rollback, disconnect or a protocol change cancels it. Submission applies no local
prediction or inventory effects, and the server still checks reach, permissions and state.

## Building typed play packets

Build common serverbound play packets through `ClientHost::build_play_packet(&PlayPacketRequest)`
(`NetworkClientHost::build_play_packet`), which routes through the client's vanilla protocol
encoder and returns `Result<ServerboundPlayPacket, PacketBuildError>`. It is available in client
host contexts including `client_setup`, reads no connection state, and applies no gameplay or
local effects. Reload and pack discovery registrations use their own scoped contexts, not a
`ClientHost`:

```rust,ignore
use mod_api::PlayPacketRequest;

let packet = host.build_play_packet(&PlayPacketRequest::UseItem {
    hand: 0,
    sequence,
    yaw,
    pitch,
})?;
host.send_packet(packet)?;
```

| Former constructor | `PlayPacketRequest` variant | Fields |
| --- | --- | --- |
| `ServerboundPlayPacket::use_item` | `PlayPacketRequest::UseItem` | `hand` (0 main, 1 off), `sequence`, `yaw`, `pitch` |
| `ServerboundPlayPacket::use_item_on` | `PlayPacketRequest::UseItemOn` | `hand`, `pos`, `face` (0 down, 1 up, 2 north, 3 south, 4 west, 5 east), `cursor` (relative to block), `inside`, `world_border`, `sequence` |
| `ServerboundPlayPacket::player_action` | `PlayPacketRequest::PlayerAction` | `action`, `pos`, `direction` (0..=5), `sequence` |
| `ServerboundPlayPacket::interact` | `PlayPacketRequest::Interact` | `entity` (network ID), `hand`, `location` (relative hit offset), `sneaking` |
| `ServerboundPlayPacket::attack` | `PlayPacketRequest::Attack` | `entity` (network ID; vanilla 26.3 uses a separate attack packet) |
| `ServerboundPlayPacket::pick_item_from_block` | `PlayPacketRequest::PickItemFromBlock` | `pos`, `include_data` |
| `ServerboundPlayPacket::pick_item_from_entity` | `PlayPacketRequest::PickItemFromEntity` | `entity`, `include_data` |
| `ServerboundPlayPacket::sign_update` | `PlayPacketRequest::SignUpdate` | `pos`, `front`, `lines` (four lines) |
| `ServerboundPlayPacket::move_player_pos` | `PlayPacketRequest::MovePlayerPos` | `pos`, `on_ground`, `horizontal_collision` |
| `ServerboundPlayPacket::move_player_pos_rot` | `PlayPacketRequest::MovePlayerPosRot` | `pos`, `yaw`, `pitch`, `on_ground`, `horizontal_collision` |
| `ServerboundPlayPacket::move_player_rot` | `PlayPacketRequest::MovePlayerRot` | `yaw`, `pitch`, `on_ground`, `horizontal_collision` |
| `ServerboundPlayPacket::move_player_status_only` | `PlayPacketRequest::MovePlayerStatusOnly` | `on_ground`, `horizontal_collision` |

Player actions follow vanilla 26.3: 0 start destroy, 1 change destroy direction, 2 abort,
3 stop destroy, 4 drop all, 5 drop one, 6 release use, 7 swap offhand, 8 stab.
Entity IDs are network IDs, not Sunrise entity handles. Angles are degrees. Sequence
numbers and the corresponding client state remain the caller's responsibility.

### Field validation and vanilla codecs

`PacketBuildError` identifies both the refused `packet` and `field`:

- **Unknown ordinals** (`UnknownAction`): `hand` outside `0..=1` (`UseItem`, `UseItemOn`,
  `Interact`), `face` or `direction` outside `0..=5` (`UseItemOn`, `PlayerAction`), and
  `action` outside `0..=8` (`PlayerAction`).
- **Packed block positions** (`OutOfRange`, field `"position"`): `pos` outside vanilla's
  `BlockPos.asLong` representation (signed 26-bit X/Z in `-33_554_432..=33_554_431`, signed
  12-bit Y in `-2048..=2047`) for `UseItemOn`, `PlayerAction`, `PickItemFromBlock` and
  `SignUpdate`.
- **Non-finite floats** (`NonFinite`): non-finite `cursor` coordinates in `UseItemOn`,
  non-finite `yaw` or `pitch` in `MovePlayerPosRot` and `MovePlayerRot`, and `NaN` in
  `MovePlayerPos` or `MovePlayerPosRot` `pos` (matching vanilla's
  `ServerGamePacketListenerImpl.containsInvalidValues`; infinite movement positions are written
  unchanged and clamped by the server, and `UseItem` writes `yaw` and `pitch` as vanilla's
  codec does).
- **Sign lines** (`SignLineTooLong`, field `"lines[i]"`): any line exceeding 384 Java UTF-16
  code units in `SignUpdate` (`ServerboundSignUpdatePacket.MAX_STRING_LENGTH`).
- **Entity interaction hit offsets**: `Interact.location` uses vanilla's lossy
  `Vec3.LP_STREAM_CODEC` (`LpVec3`), which maps `NaN` to `0.0` and clamps large finite or
  infinite values to `±1.717_986_918_3e10` rather than refusing them.

### Framing limits at publication preparation

`build_play_packet` enforces no local builder size cap. Whole-packet framing bounds are
checked when `send_packet` prepares the packet for publication on the physical connection:

- **Encoded frame limit**: at most 2,097,151 bytes (`(1 << 21) - 1`, from vanilla's
  `Varint21LengthFieldPrepender.MAX_VARINT21_BYTES = 3`) after the compression header and
  optional compression.
- **Uncompressed body limit** (when compression is enabled): at most 8,388,608 bytes (8 MiB,
  including the VarInt packet ID, from vanilla's
  `CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH = 8388608`).

Exceeding either stage fails `send_packet` with `SendPacketError::FrameTooLarge { size, limit }`.

## Container clicks

`ClientHost::container_click(&ContainerClickRequest)` builds a container click from plain
carried/changed-slot `ItemStack`s. It stays unchanged as a registry-backed host call because
the packet carries hashed stacks: the host uses the connection's item/component registry and
the game's existing component codecs and hashes. Submit the returned packet with `send_packet`;
construction applies no local menu effects. The request contains container/state IDs, slot,
button, click type, changed slots and carried stack. Click types are 0 pickup, 1 quick move,
2 swap, 3 clone, 4 throw, 5 quick craft and 6 pickup all.

Container limits are signed 32-bit container/state IDs, signed 16-bit slots, a signed-byte
button, at most 128 distinct changed slots, and the hashed component codec's count bounds.
The host refuses components it cannot encode or hash rather than silently omitting them.
No menu, reach, state or editor checks are replicated. Without a joined connection, the game host
returns `PacketBuildError::Unavailable { packet: "container_click", field: "connection" }`.
A restricted host such as the live entity-section host returns the same error with `field: "host"`
because it cannot access the connection registries.
`ServerboundPlayPacket::validate_container_click_limits` remains available for preflighting the
ID, slot and changed-slot-count wire limits.

## Packets without typed builders

For chat and commands, call `send_chat`; the game owns signing. There are no typed builders
for chat, signed commands, chat sessions, login/configuration, keep-alive, pong or custom
payload packets. Registered and engine-reserved custom-payload channels use their payload
or service API.
