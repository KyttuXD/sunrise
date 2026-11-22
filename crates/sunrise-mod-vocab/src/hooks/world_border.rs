//! Border reads carried from upstream through a level handle.
pub trait Notes {
    #[notes = r#"`level.world_border(host)` reads the bounds of the resident server level named by `level`, without opening or switching levels. Bundles need "hooks": @API:world_border@."#]
    fn world_border();
}
