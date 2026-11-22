//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;

/// The host's methods both sides share of the `resources` area, implemented by the game and reached through [`Host`].
#[sunrise_mod_api_macros::host_area]
pub trait ResourcesHost {
    /// Side: both.
    /// Open the original file from the winning pack, as `ResourceManager.getResource`
    /// and `Resource.open` do, not the bytes merged for a renderer or reload listener.
    /// In priority order, each pack can supply its own file or block lower files
    /// with `filter.block`. Client resources come from the selected asset packs
    /// over the game's assets; server data comes from the selected world data packs
    /// over the game's and mods' data, with `minecraft:vanilla` kept unless disabled.
    /// `identifier` is the resource ID (e.g. `minecraft:models/block/stone.json`
    /// or `mymod:machines/furnace.json`), without the pack's directory prefix.
    ///
    /// `Ok(None)` means the resource or stack is absent, including a filtered-out
    /// resource. Lookup or open errors are returned as [`ResourceOpenError`]; I/O
    /// and archive decoding can also fail while reading. A failure never substitutes
    /// a lower pack's bytes. The reader borrows this context's published generation.
    /// It does not reload packs, impose a resource size limit, or read the whole
    /// file on open. Use `std::io::Read::read_to_end` to explicitly keep owned bytes.
    fn open_resource(
        &self,
        pack_type: PackType,
        identifier: &Identifier,
    ) -> Result<Option<Box<dyn std::io::Read + '_>>, ResourceOpenError>;
}
