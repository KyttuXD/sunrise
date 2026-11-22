//! A player's statistics and recipe book for native server mods, through a
//! `PlayerRef` (docs/modding/src/how-to/native-mods.md): `ServerStatsCounter` and
//! `ServerRecipeBook`. Scalar reads return None for a stale or absent player;
//! a book visit reports a typed refusal. Writes return false (or None) for
//! those players, for a dead player, and for anyone but the acting player
//! during a player turn.
use super::{Game, native_server::PlayerRead, stats::Stats};
use core::ops::ControlFlow;
use mod_api::{EntityReadError, PlayerRef, StatType};

use super::native_namespaced_id::namespaced;

impl Game<'_> {
    /// Whether `player` still names the same player on this server.
    pub(super) fn native_player_current(&self, player: PlayerRef) -> bool {
        self.player_handle_read(player).is_some()
    }
    /// The guest record `player` reads from outside their turn, or None
    /// when their state is in the game (they act, or they are the host).
    pub(super) fn native_player_resting(
        &self,
        player: PlayerRef,
    ) -> Option<&sunrise_client::lan::guest::Guest> {
        match self.player_handle_read(player)? {
            super::native_server::PlayerRead::Guest(guest) => Some(guest),
            super::native_server::PlayerRead::Acting { .. } => None,
        }
    }
    /// The player's statistics, where they are kept now.
    fn native_player_stats(&self, player: PlayerRef) -> Option<Option<&Stats>> {
        if !self.native_player_current(player) {
            return None;
        }
        Some(match self.native_player_resting(player) {
            Some(g) => self.lan.as_ref()?.play.get(&g.conn).map(|p| &p.stats),
            None => Some(&self.stats),
        })
    }

    /// The value of statistic `stat_type` for `key` (`minecraft:stone`,
    /// `minecraft:jump`...), as `ServerStatsCounter.getValue`: 0 when never
    /// counted.
    pub(super) fn player_stat(
        &self,
        player: PlayerRef,
        stat_type: StatType,
        key: &str,
    ) -> Option<i32> {
        let ty = stat_type.path();
        let stats = self.native_player_stats(player)?;
        Some(stats.map_or(0, |s| s.get(ty, &namespaced(key))))
    }
    /// `Player.awardStat(stat, amount)`: adds `amount`, as the game does
    /// for its own counts, so `player_stat_award` hears it and may rewrite or cancel
    /// it, and scoreboard criteria follow. False for a player it can't
    /// reach.
    pub(super) fn player_award_stat(
        &mut self,
        player: PlayerRef,
        stat_type: StatType,
        key: &str,
        amount: i32,
    ) -> bool {
        let ty = stat_type.path();
        let key = namespaced(key);
        self.native_player_current(player)
            && self.native_as_player_handle(player, |game| game.award_stat(ty, &key, amount))
    }
    /// `Player.resetStat`: sets the statistic back to 0, which scoreboard
    /// criteria follow. False for a player it can't reach.
    pub(super) fn player_reset_stat(
        &mut self,
        player: PlayerRef,
        stat_type: StatType,
        key: &str,
    ) -> bool {
        let ty = stat_type.path();
        let key = namespaced(key);
        self.native_player_current(player)
            && self.native_as_player_handle(player, |game| game.stats.reset(ty, &key))
    }

    /// Whether the player's recipe book knows `recipe`
    /// (`ServerRecipeBook.contains`).
    pub(super) fn player_has_discovered_recipe(
        &self,
        player: PlayerRef,
        recipe: &str,
    ) -> Option<bool> {
        if !self.native_player_current(player) {
            return None;
        }
        let id = namespaced(recipe);
        Some(match self.native_player_resting(player) {
            Some(g) => g.recipe_book.recipes.contains(&id),
            None => self.level.recipe_book.recipes.contains(&id),
        })
    }
    /// Visits the player's qualified recipe ids in the resident book's order (`ServerRecipeBook.known`).
    pub(super) fn player_visit_discovered_recipes(
        &self,
        player: PlayerRef,
        visit: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<ControlFlow<()>, EntityReadError> {
        if !self.is_logical_server() {
            return Err(EntityReadError::Unavailable);
        }
        let book = match self
            .player_handle_read(player)
            .ok_or(EntityReadError::Stale)?
        {
            PlayerRead::Guest(guest) => &guest.recipe_book.recipes,
            PlayerRead::Acting { .. } => &self.level.recipe_book.recipes,
        };
        Ok(book.iter().map(String::as_str).try_for_each(visit))
    }
    /// `ServerPlayer.awardRecipes`, as `/recipe give` does: the recipes
    /// unlock in the player's book, with their toasts and the
    /// `player_recipe_discovered` hook, and the client hears of them.
    /// Unknown ids are skipped. The number of recipe displays added, None
    /// for a player it can't reach.
    pub(super) fn player_award_recipes(
        &mut self,
        player: PlayerRef,
        recipes: &[&str],
    ) -> Option<i32> {
        self.native_player_change_recipes(player, true, recipes)
    }
    /// `ServerPlayer.resetRecipes`, as `/recipe take` does. The number of
    /// recipe displays taken out, None for a player it can't reach.
    pub(super) fn player_reset_recipes(
        &mut self,
        player: PlayerRef,
        recipes: &[&str],
    ) -> Option<i32> {
        self.native_player_change_recipes(player, false, recipes)
    }
    fn native_player_change_recipes(
        &mut self,
        player: PlayerRef,
        give: bool,
        recipes: &[&str],
    ) -> Option<i32> {
        if !self.native_player_current(player) {
            return None;
        }
        // `awardRecipes` takes recipes the server has: an id no recipe has
        // would otherwise go into the book.
        let index = &self.stations().recipe_index;
        let ids: Vec<String> = recipes
            .iter()
            .map(|r| namespaced(r))
            .filter(|id| index.contains(id))
            .collect();
        let mut count = 0;
        self.native_as_player_handle(player, |game| {
            count = game.change_recipes(give, &ids);
        })
        .then_some(count)
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;

    #[test]
    #[ignore = "daily"]
    fn a_resting_guest_reads_from_their_own_record() {
        use std::sync::{Arc, atomic::AtomicI32, mpsc};
        use sunrise_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
        use sunrise_protocol::packets::configuration::ClientInformation;

        with_game(|game| {
            let settings = super::super::lan::LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            };
            let net = NetConfig {
                announce: false,
                ..NetConfig::lan(0, "StatsTest".into(), 2)
            };
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 10,
                simulation_distance: 10,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
                level_seeds: Default::default(),
            };
            game.lan_host(settings, net, config).unwrap();
            let (tx, _rx) = mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: 1,
                    uuid: 123,
                    name: "guest".into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::default()),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                    expected: false,
                },
                20,
                [0.0; 3],
                8,
            );
            guest.placed = true;
            guest.recipe_book.recipes.insert("minecraft:stick".into());
            let host = game.lan.as_mut().unwrap();
            host.hub.guests.push(guest);
            let mut play = super::super::lan_actions::GuestPlay::default();
            play.stats.add("custom", "minecraft:jump", 7);
            host.play.insert(1, play);
            game.stats.add("custom", "minecraft:jump", 3);

            let player = game.player(123).unwrap();
            let jump = |game: &super::Game<'_>| game.player_stat(player, StatType::Custom, "jump");
            assert_eq!(jump(game), Some(7));
            assert_eq!(
                game.player_has_discovered_recipe(player, "stick"),
                Some(true)
            );
            assert_eq!(
                game.player_has_discovered_recipe(player, "minecraft:diamond_pickaxe"),
                Some(false)
            );
            let mut recipes = Vec::new();
            assert_eq!(
                player.visit_discovered_recipes(game, &mut |id| {
                    recipes.push(id.to_owned());
                    ControlFlow::Continue(())
                }),
                Ok(ControlFlow::Continue(()))
            );
            assert_eq!(recipes, ["minecraft:stick"]);
            // In the guest's turn their state is in the game, not merged with the resting book.
            game.level.recipe_book.recipes.clear();
            game.lan_acting_uuid = Some(123);
            game.lan_acting_entity = Some(20);
            assert_eq!(jump(game), Some(3));
            assert_eq!(
                player.visit_discovered_recipes(game, &mut |_| panic!("empty acting book")),
                Ok(ControlFlow::Continue(()))
            );
            let book = &mut game.level.recipe_book.recipes;
            book.insert("minecraft:torch".into());
            book.insert("minecraft:oak_planks".into());
            let mut first = Vec::new();
            assert_eq!(
                player.visit_discovered_recipes(game, &mut |id| {
                    first.push(id.to_owned());
                    ControlFlow::Break(())
                }),
                Ok(ControlFlow::Break(()))
            );
            assert_eq!(first, ["minecraft:oak_planks"]);
            game.lan_acting_uuid = None;
            game.lan_acting_entity = None;
            // A guest without play state yet has counted nothing.
            game.lan.as_mut().unwrap().play.remove(&1);
            assert_eq!(jump(game), Some(0));
            game.lan.as_mut().unwrap().hub.guests.remove(0);
            assert_eq!(
                player.visit_discovered_recipes(game, &mut |_| panic!("stale player")),
                Err(EntityReadError::Stale)
            );
        });
    }
}
