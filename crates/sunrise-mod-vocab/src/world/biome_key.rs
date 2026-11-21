//! A biome's registry identity and its borrowed view.
use std::sync::Arc;

/// The canonical identifier of a registered biome (`ResourceKey<Biome>`).
/// Compare keys by name, not by a chunk palette slot or a network id. A key may
/// be retained across world loads; it does not keep any world's biome data alive.
/// Obtain it from [`BiomeRef::key`].
// no wire: native registry identity; save and network boundaries encode their own names or ids.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BiomeKey(Arc<str>);

impl BiomeKey {
    /// The full, namespaced id of this biome, independent of any installed world.
    pub fn id(&self) -> &str {
        &self.0
    }
}

/// A registered biome borrowed from one loaded registry (`Holder<Biome>`).
/// The borrow cannot outlive its registry scope. Comparison uses the canonical
/// id; it does not compare the loaded definitions. Retain a [`BiomeKey`] if
/// you need the identity after the borrow ends.
// no wire: borrows the actual loaded registry in-process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BiomeRef<'a>(&'a Arc<str>);

impl<'a> BiomeRef<'a> {
    /// The full, namespaced id held by this registry entry.
    pub fn id(self) -> &'a str {
        self.0
    }

    /// Retain the canonical key beyond the registry borrow without copying its text.
    pub fn key(self) -> BiomeKey {
        BiomeKey(Arc::clone(self.0))
    }
}

/// Lend a canonical name from an installed biome registry.
// hidden: only engine adapters can issue a view of a registered biome.
#[doc(hidden)]
pub fn biome_ref(registered_name: &Arc<str>) -> BiomeRef<'_> {
    BiomeRef(registered_name)
}
