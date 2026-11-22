//! What a server mod reads of the tick times and sets of the view and simulation distance.
use super::Game;

impl Game<'_> {
    /// `MinecraftServer.getTickTimesNanos`.
    pub(super) fn tick_times(&self) -> Vec<u64> {
        self.clock.tick_times()
    }
    /// `PlayerList.getViewDistance` on either logical server, including unpublished worlds.
    pub(super) fn server_view_distance(&self) -> Option<i32> {
        if !self.is_logical_server() {
            return None;
        }
        Some(self.lan.as_ref()?.hub.config.view_distance)
    }
    /// `PlayerList.getSimulationDistance` on either logical server, including unpublished worlds.
    pub(super) fn server_simulation_distance(&self) -> Option<i32> {
        if !self.is_logical_server() {
            return None;
        }
        Some(self.lan.as_ref()?.hub.config.simulation_distance)
    }
    /// `PlayerList.setViewDistance` on either server. The native range is
    /// `ChunkMap.MIN_VIEW_DISTANCE` through `MAX_VIEW_DISTANCE` in `setServerViewDistance`.
    pub(super) fn server_set_view_distance(&mut self, distance: i32) -> bool {
        let reason = if !(2..=32).contains(&distance) {
            "expected a distance in 2..=32"
        } else if self.set_view_distance(distance) {
            return true;
        } else {
            "no logical server player list"
        };
        let mod_id = mod_api::content::crash::current_native_mod().unwrap_or("unknown");
        log::warn!("mod {mod_id}: server_set_view_distance refused {distance}: {reason}");
        false
    }
    /// `PlayerList.setSimulationDistance` on either server. The native range spans the
    /// supported endpoints of `Options.simulationDistance`'s `IntRange` (2 with
    /// `DEBUG_ALLOW_LOW_SIM_DISTANCE`, up to 32); `DistanceManager.updateSimulationDistance`
    /// applies the distance to tickets rather than validating the option.
    pub(super) fn server_set_simulation_distance(&mut self, distance: i32) -> bool {
        let reason = if !(2..=32).contains(&distance) {
            "expected a distance in 2..=32"
        } else if self.set_simulation_distance(distance) {
            return true;
        } else {
            "no logical server player list"
        };
        let mod_id = mod_api::content::crash::current_native_mod().unwrap_or("unknown");
        log::warn!("mod {mod_id}: server_set_simulation_distance refused {distance}: {reason}");
        false
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        execution_role::ExecutionRole, integrated::Serving,
        native_entity_refs::tests::fixture_root, native_guest_turn_tests::with_guests_for_role,
    };
    use super::Game;
    use mod_api::ServerHost;

    #[test]
    #[ignore = "daily"]
    fn a_mod_reads_and_sets_the_servers_distances() {
        crate::game::in_own_process(
            "game::native_server_distances::tests::a_mod_reads_and_sets_the_servers_distances",
            || {
                let advertised = |game: &Game<'_>| {
                    let config = &game.lan.as_ref().unwrap().hub.config;
                    (config.view_distance, config.simulation_distance)
                };
                let ticket_settings = |game: &Game<'_>| {
                    (
                        game.settings.render_distance,
                        game.settings.simulation_distance,
                    )
                };
                for role in [
                    ExecutionRole::IntegratedServer,
                    ExecutionRole::DedicatedServer,
                ] {
                    with_guests_for_role(fixture_root(), role, |game| {
                        if role == ExecutionRole::IntegratedServer {
                            game.serving = Some(Serving::fixture(0));
                            assert_eq!(game.published_port(), None);
                        }
                        game.clock = crate::clock::FixedStep::default();
                        for took in [40_000_000, 51_000_000, 62_000_000] {
                            game.clock.record(std::time::Duration::from_nanos(took));
                        }
                        let host: &mut dyn ServerHost = game;
                        let times = host.tick_times();
                        assert_eq!(times.len(), 100);
                        assert_eq!(times[..4], [40_000_000, 51_000_000, 62_000_000, 0]);
                        assert_eq!(host.server_view_distance(), Some(2));
                        assert_eq!(host.server_simulation_distance(), Some(2));
                        assert!(host.server_set_view_distance(6), "{role:?}");
                        assert!(host.server_set_simulation_distance(5), "{role:?}");
                        for outside in [1, 33] {
                            assert!(!host.server_set_view_distance(outside));
                            assert!(!host.server_set_simulation_distance(outside));
                        }
                        assert_eq!(host.server_view_distance(), Some(6));
                        assert_eq!(host.server_simulation_distance(), Some(5));
                        assert_eq!(advertised(game), (6, 5));
                        assert_eq!(ticket_settings(game), (6, 5));
                        let hub = &game.lan.as_ref().unwrap().hub;
                        assert_eq!(hub.guest(12).unwrap().view_distance, 6);
                        assert_eq!(hub.guest(13).unwrap().view_distance, 6);

                        let player_list = game.lan.take();
                        let host: &mut dyn ServerHost = game;
                        assert_eq!(host.server_view_distance(), None);
                        assert_eq!(host.server_simulation_distance(), None);
                        assert!(!host.server_set_view_distance(7));
                        assert!(!host.server_set_simulation_distance(8));
                        assert_eq!(ticket_settings(game), (6, 5));

                        // Even a Game with a hub cannot act as the server from the client role.
                        game.lan = player_list;
                        game.options.execution_role = ExecutionRole::Client;
                        let host: &mut dyn ServerHost = game;
                        assert_eq!(host.server_view_distance(), None);
                        assert_eq!(host.server_simulation_distance(), None);
                        assert!(!host.server_set_view_distance(7));
                        assert!(!host.server_set_simulation_distance(8));
                        assert_eq!(advertised(game), (6, 5));
                        assert_eq!(ticket_settings(game), (6, 5));
                    });
                }
            },
        );
    }
}
