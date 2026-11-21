# Field of view and name tags

Two client hooks change what the local player sees without drawing anything yourself, and a world draw hook can
add labels of its own.

## Field of view

`compute_fov_modifier` is NeoForge's `ComputeFovModifierEvent`. It runs as `Camera.tickFov` asks the
player the camera follows for its modifier (the local player or a spectated player). A non-player
camera uses 1 without firing the hook. First-person spyglass use returns 0.1 before the hook, as in
`AbstractClientPlayer.getFieldOfViewModifier`.

| `ComputeFovModifierEvent` field | Meaning |
| --- | --- |
| `player` | The viewpoint player's flying, movement and item-use inputs (`FovPlayer`) |
| `base` | Vanilla's raw modifier, after movement and item use but before the FOV Effects option |
| `effect_scale` | The FOV Effects option used to blend from 1 to `base` |
| `modifier` | The blended target; edit it before vanilla's tick easing and 0.1–1.5 range |

`compute_fov` is NeoForge's `ViewportEvent.ComputeFov`. It runs after the field of view has been
calculated, including death and fluid effects, and before projection and nausea distortion. The world
angle follows the FOV option and the eased modifier; the first-person hand angle starts at 70 degrees.
The event gives `camera` (the viewpoint position), `partial` (its interpolation fraction), `projection`
(`mod_api::client::camera::FovProjection::World` or `Hud`), and `scoping` (the viewpoint player is using
a spyglass in first person). Edit `degrees` to change just that projection. Terrain and overlays read
the extracted angle without firing the hook again. The title panorama and independent free camera do
not fire it.

Both hooks pass the last accepted value to the next listener. A non-finite edit is reported and does
not replace that value. A panic is logged and shown as an error toast naming the mod; that callback
is disabled for the session and the other listeners still run.

### Compose a zoom

1. Register a [key mapping](../native-mods/13-runtime-packages-and-identity.md#registered-key-mappings). Resolve it
   during `client_setup` with `host.mod_key_mapping(owner, id)` and keep the `ModKeyMappingRef`.
   In `client_tick_post` or `render_frame_pre`, read `mapping.is_down(host)` and `host.playing()` and
   update the held state and animation in your mod. These callbacks also work while the camera
   follows a non-player, where `compute_fov_modifier` is not called.
2. For vanilla easing, divide `event.modifier` by your zoom factor in `compute_fov_modifier`.
   For an instant zoom or your own animation, divide `event.degrees` in `compute_fov` when
   `event.projection == FovProjection::World`. Use `event.partial` to interpolate between ticks;
   leave the angle alone when `event.scoping` if you want the spyglass to take precedence. Apply
   your zoom in one hook, not both.
3. Scale `event.yaw_delta` and `event.pitch_delta` in [`adjust_player_turn`](camera-and-collision-reads.md#camera-snapshots-and-perspective)
   while zoomed. This applies after device-specific processing and leaves the options unchanged.

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
