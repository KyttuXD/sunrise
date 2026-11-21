# Field of view and name tags

Camera hooks change the world projection without drawing anything yourself, and a world draw hook can
add labels of its own.

## Field of view

`compute_fov_modifier` runs once per client tick when a player is the viewpoint, including when spectating
another player. It is NeoForge's `ComputeFovModifierEvent`. The hook does not fire for a non-player camera.
It runs after vanilla's player effects, including the first-person spyglass, but before `Camera.tickFov`
eases and clamps the modifier to vanilla's `0.1..=1.5`.

| `ComputeFovModifierEvent` field | Meaning |
| --- | --- |
| `player` | The viewpoint's calculation inputs (`FovPlayer`), including the item in use |
| `base` | The scale before any mod ran |
| `modifier` | The scale to ease towards; later listeners see the last accepted answer |

`compute_fov` runs at world camera extraction, also for non-player viewpoints, after modifier interpolation,
death and camera fluid, before projection and Nausea. `ComputeFovEvent.camera` is the actual world-space
camera position, `partial` is its interpolation fraction, and `degrees` is the world angle to edit.
It does not set the hand/HUD angle or the title panorama. Both hooks report non-finite edits and keep the
preceding answer.

### Composing a zoom

Register a [key mapping](../native-mods/13-runtime-packages-and-identity.md#registered-key-mappings),
resolve it in `client_setup` with `host.mod_key_mapping(owner, id)`, and keep the returned mapping.
Read `mapping.is_down(host)` and `host.playing()` in `client_tick_post` or `render_frame_pre`.
Keep the held state and any easing in your own client state.

For vanilla tick easing, divide `event.modifier` by your zoom factor in `compute_fov_modifier`. For an
instant change or your own animation, instead divide `event.degrees` in `compute_fov` by the factor
interpolated using `event.partial`. Don't divide in both hooks for the same zoom. If your effect should
yield to the spyglass, check for `UseItem::Spyglass` in `ComputeFovModifierEvent.player.using` with
`host.camera_type().is_first_person()`; use the viewpoint state, not the local player's off-camera
item. `host.camera_entity()` identifies the current viewpoint.

To adjust mouse and controller turning together, multiply both deltas by your mod's turn scale in
[`adjust_player_turn`](camera-and-collision-reads.md#camera-snapshots-and-perspective). Use
`calculate_player_turn` only for mouse sensitivity or cinematic smoothing for that turn.

## Name tags

`render_name_tag` runs when the game decides whether to show the name tag of a player, mob, armor stand or item
frame, before the tag is drawn. It is NeoForge's `RenderNameTagEvent`.

| `RenderNameTagEvent` field | Meaning |
| --- | --- |
| `entity_id`, `kind` | The entity and its type id |
| `vanilla_visible` | What vanilla decided |
| `visible` | Set it to show or hide the tag |
| `text` | Set it to `Some(text)` to replace the tag's text |

Distance and the entity's availability still apply. The local player's own tag is considered only when its camera
and settings would show it.

## Labels in the world

A world draw hook (`render_level_stage` or `submit_custom_geometry`) or a [registered debug renderer
callback](hud-drawing.md#registering-world-space-debug-renderers) can draw a label with `draw.name_tag`,
as vanilla draws a name tag: half a block above the position, facing the camera. Use the
callback's `draw` argument, or `event.draw` from a world draw hook.

```rust,ignore
let options = NameTagOptions::new(false, true, 0xF0);
let _ = draw.name_tag([x + 0.5, y + 1.0, z + 0.5], &Text::literal("Furnace"), options);
```

| `NameTagOptions` field | Meaning |
| --- | --- |
| `see_through` | Off: drawn once, faded and translucent, hidden behind walls (a sneaking name). On: opaque in the world and a faded copy over everything, walls included |
| `background` | The black backdrop behind the faded text, at the player's text background opacity |
| `light` | The light at the label: sky light in the high four bits, block light in the low four (`0xF0` full sky, `0x0F` full block) |
| `faded_alpha` | `Some(a)` (`with_faded_alpha(a)`) sets the alpha byte of the faded text, in either copy, in place of the value derived from the text background opacity. An alpha under 26 draws no text |
| `exact_light` | `with_exact_light()`: with `see_through`, the copy in the world is drawn in `light` exactly, without the floor of 2 |

Without `see_through` the label is drawn in exactly `light`. With it, the copy in the world raises the sky and block
levels to at least 2 each (unless `exact_light`), and the copy over everything is unlit. The backdrop's alpha is 255 times the player's text
background opacity (0.25 when the background is for chat only), and the faded text's is 255 times the larger of
(opacity + 0.75) / 2 and 0.5: 0x3F and 0x7F at the defaults. Labels are drawn with the world's name tags, far to near.

A label is 0.025 blocks a font pixel, so it shrinks with distance; scale by distance yourself for a constant size.
Its quads share the view's 4,096 with `quads`. A position that is not finite, text deeper than 512 component nodes
and a label past the quads left are refused by name (`DrawError`) and draw nothing. The
[hook notes](../../reference/hook-notes.md#custom-world-geometry) give the details.

## The client session

For a HUD that shows session information, read `ClientHost::client_session()`, and the local `PlayerRef`'s
`block_breaking` and `ambient_mood`. Values that may be missing (a session, an external display name) are
`Option`s, and every counter stays on the client. `client_server()` lends `name`, the
redacted `address` and `save_folder` as `Option<&str>`. Copy a field with
`.map(str::to_owned)` only to keep it beyond the host borrow. To compare the applied
language, use `host.client_session().client_language().as_deref() == Some("en_us")`.
Keep the `Arc<str>` if you need that snapshot across a language change. The
[hook notes](../../reference/hook-notes.md#client-session-and-local-player-status) give
the exact availability and timing.
