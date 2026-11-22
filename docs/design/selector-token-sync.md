# Selector-token synchronization

Sunrise sends `sunrise:selector_tokens` immediately before each play command tree to a
Sunrise peer. Vanilla command-tree bytes are unchanged. The channel is reserved for the
engine; a mod cannot declare it as a raw payload. The snapshot contains parsing and
completion metadata, never the server's callable selector implementation.

## Revision 1 layout

All integer and string encodings below are the protocol's standard encodings. Fixed-width
integers are big-endian. A string is a VarInt UTF-8 byte length followed by those bytes.

| Field | Encoding |
| --- | --- |
| Revision | One unsigned byte, exactly `1` |
| Token count | Nonnegative VarInt |
| Name, for each token | String, Brigadier unquoted-string characters (`[0-9A-Za-z_.+-]`) |
| Tooltip translation key | String |
| Players only | Boolean byte |
| Has limit | Boolean byte |
| Limit, if present | Unsigned 32-bit integer |
| Sort | One byte: `0` arbitrary, `1` nearest, `2` furthest, `3` random |

Names are unique and cannot replace vanilla's `p`, `a`, `r`, `s`, `e` or `n`. An absent
limit means unbounded. Names and tooltip keys follow vanilla's `STRING_UTF8` bound (32,767
UTF-16 code units). No trailing bytes are accepted.

## Bounds and publication

Framing follows vanilla's `ClientboundCustomPayloadPacket.MAX_PAYLOAD_SIZE` (1,048,576
bytes, 1 MiB) and `STRING_UTF8` (32,767 UTF-16 code units) at the network boundary, with
no separate registration-time metadata budget. When encoding a snapshot, any token whose
name or tooltip exceeds 32,767 UTF-16 code units or would push the encoded payload past
1,048,576 bytes is logged and skipped while valid tokens that fit are kept. The receiver
enforces the same 1,048,576-byte payload bound before decoding.

The receiver validates a complete snapshot before replacing its command registry metadata.
Starting another connection clears the previous server's tokens. A client registry rebuild
keeps the current server's token snapshot rather than substituting locally installed mods.

## Revision handling

The server emits revision 1 only. There is no unversioned format, older-format fallback or
alias channel. An unknown revision disables selector metadata for that connection, clears
any prior token snapshot and emits one log line naming the unsupported revision. Further
selector metadata for that connection is ignored, including a later revision 1 snapshot.
A new connection starts with revision support enabled again. Unknown revisions do not
panic or disconnect the client; malformed revision 1 data is a protocol error.
