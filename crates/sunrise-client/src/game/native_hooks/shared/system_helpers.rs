use super::*;

impl Game<'_> {
    pub(in crate::game) fn native_block_collision_in(
        &self,
        world: &sunrise_client::world::World,
        pos: [i32; 3],
    ) -> Vec<[f64; 6]> {
        self.block_collision_shape_in(world, pos)
            .boxes()
            .iter()
            .map(|bounds| {
                let min = bounds.min_position();
                let max = bounds.max_position();
                [min.x, min.y, min.z, max.x, max.y, max.z]
            })
            .collect()
    }
    /// The areas the game itself draws beside the open screen's window: the
    /// container build's, or the frame's own arithmetic with no build yet.
    pub(super) fn screen_own_exclusion(&self, batch: &HudBatch) -> Vec<[f32; 4]> {
        if !matches!(self.screen, Screen::Inventory | Screen::Creative) {
            return Vec::new();
        }
        match self.built_container(batch) {
            Some(layout) => layout.exclusion(),
            None => self.container_layout(batch).exclusion(),
        }
    }
    /// A native mod's own folder: `sunrise-mods/.sunrise-mod-data/<id>/`. Not created here.
    pub(in crate::game) fn native_data_dir(&self, mod_id: &str) -> PathBuf {
        Self::mod_data_root().join(mod_id)
    }
    pub(in crate::game) fn mod_data_root() -> PathBuf {
        Self::game_dir()
            .join(mod_list::DIR)
            .join(".sunrise-mod-data")
    }
    /// The game's `config/` folder beside `options.txt`, shared by every mod
    /// as on Fabric and NeoForge. The same folder on the window, its server
    /// thread and a LAN host; a dedicated server's is in its own folder,
    /// beside `server.properties`. Made when missing. Mods reach their own
    /// files in it through `Host::config_files`.
    pub(in crate::game) fn native_config_dir() -> std::io::Result<PathBuf> {
        config_dir(&Self::game_dir())
    }
    /// Shows these recipes in the panel instead of its items, as many as
    /// fit a page, under `title` and a back button, with a row of `tabs`
    /// (each an icon; `tab` is the selected one) between them. At most 256
    /// recipes and 16 tabs; no recipes shows the items again. The same title
    /// and tab again keep the page the player turned to.
    pub(in crate::game) fn native_panel_recipes(
        &mut self,
        mod_id: &str,
        title: &str,
        tabs: Vec<mod_api::Stack>,
        tab: u32,
        views: Vec<NativeRecipeView>,
    ) {
        let page = RecipePage::new(title, tabs, tab, views);
        self.panel.show_recipes(mod_id, page);
    }
    /// No world is open: a client without a player (the title screen). A
    /// server's `Game` (the server thread, a dedicated server, a LAN host)
    /// has no player of its own, and is always in its world.
    pub(super) fn outside_world(&self) -> bool {
        self.player.is_none() && self.options.execution_role.is_client()
    }
}
