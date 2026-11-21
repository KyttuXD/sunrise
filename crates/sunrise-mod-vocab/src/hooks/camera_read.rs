pub trait Notes {
    #[notes = r#"## Camera entity

`ClientHost::camera_entity()` returns an `EntityRef` for the entity the view follows
(`Minecraft.getCameraEntity`): the local player, unless a server attached the camera to another
entity. It is `None` on the title screen and outside a world. Together with `camera()` and
`camera_type()` it lets a minimap or freecam mod tell its own view from a spectated one.

`ClientHost::set_camera_entity(entity)` is `Minecraft.setCameraEntity`: it moves the view onto a
mob (or back onto the player with the player's own ref) and returns whether it did. It selects
the entity only; the camera angle stays the entity's, and `camera_pose` is the way to override it.

## Client heights

`level.visit_surface_heights(host, columns, heightmap, &mut visit)` reads `LevelReader.getHeight`
on this client's displayed level. It passes each `(x, z)` and its optional height to `visit` in
input order, with `None` for a chunk the server hasn't sent this window. Return
`ControlFlow::Break(())` to stop early. The host does not build a result list."#]
    fn camera_read_note0();
}
