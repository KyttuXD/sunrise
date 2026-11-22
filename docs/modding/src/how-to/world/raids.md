# Raid handles

Use `level.raid(host, id)`, `level.raid_at(host, pos)` and
`level.visit_raids(host, &mut visit)` to obtain `RaidRef` handles. The visit follows
that level's raid-map order and includes stopped raids until their next tick removes
them. Handles resolve their own loaded server level even while another dimension
is in place. Raid reads, omen-level writes and `stop` do not swap levels.

- `raid.visit_raiders(host, &mut visit)` visits the living members in the level's mob-list order.
- `raid.visit_wave_raiders(host, wave, &mut visit)` visits that wave's living members in the same order.
- `raid.visit_heroes(host, &mut visit)` visits the recorded hero UUIDs in set order, including offline players.

Return `ControlFlow::Break(())` to stop visiting (`Ok(Break(()))`), or continue until
`Ok(Continue(()))`, including when there is nothing to visit. If you need an owned list,
collect the handles or UUIDs you need in the visitor.

Refused visits do not call the visitor. A closed level or earlier world epoch returns
`LevelReadError::Stale` for a level visit, or `RaidReadError::Stale` for a raid visit. A
missing raid in a live level is also `RaidReadError::Stale`. An unreadable side, phase
or raid map returns the corresponding `Unavailable` error.

Raid ids are local to a level. Member handles from a parked raid keep their identity;
their ordinary entity operations require the entity's level to be in place.
