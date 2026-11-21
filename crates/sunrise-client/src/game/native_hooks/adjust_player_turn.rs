use super::*;

impl Game<'_> {
    /// The local player's relative input after device processing and before `Entity.turn`.
    pub(in crate::game) fn native_adjust_player_turn(
        &self,
        yaw_delta: &mut f64,
        pitch_delta: &mut f64,
    ) {
        self.fire_adjust_player_turn(yaw_delta, pitch_delta);
    }
}

#[cfg(test)]
mod tests {
    use mod_api::{AdjustPlayerTurnEvent, ClientHost, HookId, Hooks, ModEntry};
    use sunrise_client::motion::PlayerMotion;
    use sunrise_core::geometry::Vec3;

    #[test]
    #[ignore = "daily"]
    fn relative_turns_keep_accepted_values_when_a_listener_fails() {
        struct Adjust(u8);
        #[mod_api::__subscriptions]
        impl Hooks for Adjust {
            fn adjust_player_turn(&self, _: &dyn ClientHost, event: &mut AdjustPlayerTurnEvent) {
                match self.0 {
                    0 => event.yaw_delta *= 2.0,
                    1 => {
                        event.yaw_delta = 1.0;
                        event.pitch_delta = f64::MAX;
                    }
                    2 => {
                        event.yaw_delta = 1.0;
                        panic!("turn callback fixture");
                    }
                    _ => event.pitch_delta = -event.pitch_delta,
                }
            }
        }

        let mods = ["scale", "invalid", "panics", "invert"]
            .into_iter()
            .zip(0..)
            .map(|(id, step)| ModEntry::new(id, Box::new(Adjust(step))))
            .collect::<Vec<_>>();
        mod_platform::testing::with_mods(mods.into(), || {
            crate::game::native_entity_refs::tests::with_client_game(|game| {
                game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
                game.yaw = 0.0;
                game.pitch = 0.0;
                game.look_turn(20.0, 20.0);
                assert_eq!((game.yaw, game.pitch), (6.0, -3.0));
                assert_eq!(
                    mod_api::__private::poisoned_listeners(HookId::AdjustPlayerTurn),
                    1
                );
                game.turn_player(20.0, 20.0);
                assert_eq!((game.yaw, game.pitch), (12.0, -6.0));
            });
        });
    }
}
