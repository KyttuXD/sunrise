use super::*;
/// What answers an entity capability for one entity type: given the entity and the lookup's
/// context (its [`EntityCapabilityKey`]'s `C`), its handler, or none to leave it to the next
/// provider. A closure is one.
pub trait EntityCapabilityProvider<T: ?Sized, C>: Send + Sync {
    /// The handler for `entity`, asked with `context`, or none.
    fn provide(&self, host: &mut dyn ServerHost, entity: EntityRef, context: C) -> Option<Box<T>>;
}
impl<T: ?Sized, C, F> EntityCapabilityProvider<T, C> for F
where
    F: Fn(&mut dyn ServerHost, EntityRef, C) -> Option<Box<T>> + Send + Sync,
{
    fn provide(&self, host: &mut dyn ServerHost, entity: EntityRef, context: C) -> Option<Box<T>> {
        self(host, entity, context)
    }
}
// no wire: trait-object alias inherits its provider trait policy; callbacks are not serialized.
/// An entity capability provider, as the registrar keeps it.
pub type EntityProvider<T, C = ()> = dyn EntityCapabilityProvider<T, C>;

/// Run the engine's preparation only after the subscription index finds Parts of this type.
pub(crate) fn with_entity_part_calls<R>(
    hook: HookId,
    name: &'static str,
    kind: &str,
    visit: impl FnOnce(EntityPartCalls<'_>) -> R,
) -> Option<R> {
    if !has_scoped_hook(hook) {
        return None;
    }
    let table = parts(hook, name);
    let parts = table.for_hook(hook)?.entity(|| Some(kind));
    if parts.is_empty() {
        return None;
    }
    if matches!(&table, PartsRef::Shared(_))
        && !GAME_PARTS.with_borrow(|lookup| lookup.game_thread)
        && crate::dispatch_scope::backend()
            .is_none_or(|backend| (backend.diagnostics.first_off_thread_warning)())
    {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        log::warn!("Entity Part callback {name} asked off a game thread ({thread})");
    }
    Some(visit(EntityPartCalls {
        parts: parts.iter(),
        hook,
        name,
    }))
}

// no wire: dispatch iterates borrowed, process-local Part registrations.
// exhaustive: the iterator is built only by the scoped dispatcher.
/// The registered Parts for one callback and entity type, in dispatch order.
#[doc(hidden)]
pub struct EntityPartCalls<'a> {
    parts: std::slice::Iter<'a, Attached<dyn EntityPart>>,
    hook: HookId,
    name: &'static str,
}

impl<'a> Iterator for EntityPartCalls<'a> {
    type Item = EntityPartCall<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(EntityPartCall {
            registration: self.parts.next()?,
            hook: self.hook,
            name: self.name,
        })
    }
}

// no wire: the callback borrows an installed Part registration.
// exhaustive: the call is built only by the scoped dispatcher.
/// One registered Part and the particular callback being dispatched.
#[doc(hidden)]
pub struct EntityPartCall<'a> {
    /// The Part's owner and identity, for registrations and paired decisions.
    pub registration: &'a Attached<dyn EntityPart>,
    hook: HookId,
    name: &'static str,
}

impl EntityPartCall<'_> {
    /// Whether this subscriber is the same registration, on the same entity type, that
    /// answered a paired callback. Two registrations can share one Part instance.
    pub fn is_registration(&self, previous: &Attached<dyn EntityPart>) -> bool {
        self.registration.mod_index == previous.mod_index
            && self.registration.order == previous.order
    }

    /// Invoke the subscribed method. A phase refusal is reported every time; a panic disables
    /// this mod's callback for this moment, without disabling other mods or other callbacks.
    pub fn invoke<R>(self, invoke: impl FnOnce(&dyn EntityPart) -> R) -> Option<R> {
        let attached = self.registration;
        let id = attached.mod_id;
        if !crate::mod_index_on(attached.mod_index) || crate::runtime::poisoned(self.hook, id) {
            return None;
        }
        let _guard = crate::content::crash::enter_native_mod(crate::content::crash::ModCall {
            id,
            hook: self.name,
            part: Some(attached.type_name),
        });
        match crate::callback_owner::contain_unwind(|| {
            crate::runtime::refusable(crate::runtime::outside_world(), || invoke(&*attached.part))
        }) {
            Ok(Ok(answer)) => Some(answer),
            Ok(Err(refusal)) => {
                refusal.report_each();
                None
            }
            Err(message) => {
                crate::runtime::poison(self.hook, id);
                log::error!(
                    "mod {id}: entity Part callback {} panicked and is disabled for this session: {message}",
                    self.name
                );
                crate::callback_owner::record_callback_panic(id, "entity Part", self.name, message);
                None
            }
        }
    }
}

impl HookParts {
    pub(crate) fn entity<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn EntityPart>] {
        attached(&self.entities, id)
    }
}

/// Whether any mod registered a provider of entity capability `key`, for any entity type: one
/// table lookup, no allocation, so the game's own lookups skip all their work where none did.
pub fn has_entity_providers<T: ?Sized + 'static, C: 'static>(
    key: &EntityCapabilityKey<T, C>,
) -> bool {
    installed()
        .and_then(|installed| installed.keys.entity_capabilities.get(key.name))
        .is_some_and(|by_kind| !by_kind.is_empty())
}

/// The providers mods registered for `key` and the type of `entity`, for the game's own
/// lookups (a hopper's): none where no mod registered one, so a vanilla container entity's
/// lookup costs one table check and allocates nothing, and the game needn't work out its own
/// answer. [`EntityProviders::ask`] then asks them, as the game's lookups for block entities do.
pub fn entity_providers<T: ?Sized + 'static, C: Copy + 'static>(
    key: &EntityCapabilityKey<T, C>,
    host: &dyn Host,
    entity: EntityRef,
    context: C,
) -> Option<EntityProviders<T, C>> {
    let by_kind = installed()?.keys.entity_capabilities.get(key.name)?;
    let name = entity.kind(host)?.id(host)?;
    let parts = by_kind.get(name.as_str())?;
    Some(EntityProviders {
        parts,
        entity,
        context,
        handler: PhantomData,
    })
}

// no wire: provider lookup borrows static capability dispatch tables and process-local type identity.
/// An entity's providers of one capability ([`entity_providers`]).
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct EntityProviders<T: ?Sized + 'static, C: 'static> {
    pub(crate) parts: &'static [CapabilityPart<EntityProviderOf>],
    pub(crate) entity: EntityRef,
    pub(crate) context: C,
    pub(crate) handler: PhantomData<fn() -> Box<T>>,
}

impl<T: ?Sized + 'static, C: Copy + 'static> EntityProviders<T, C> {
    /// A mod's handler where the game would answer with its own unless a mod comes first: one
    /// registered `before_game`, else, where the game has no answer (`game`), the first mod's.
    pub fn ask(self, host: &mut dyn ServerHost, game: bool) -> Option<Capability<T>> {
        let (entity, context) = (self.entity, self.context);
        match ask_providers(
            host,
            |index| next_provider(self.parts, index),
            EntityProviderOf::get::<T, C>,
            |provider, host| provider.provide(host, entity, context),
            |_| game,
        ) {
            Answer::Mod(found) => Some(found),
            Answer::Game | Answer::Nobody => None,
        }
    }
}

impl dyn Registrar + '_ {
    /// Attach `part` to the entity type `entity` (`minecraft:zombie`): its
    /// [`EntityPart::register_goals`] runs at every `Mob` construction of that
    /// type, spawning or loading, and its goals join the mob's selectors
    /// beside vanilla's. Its `general_entity_tick` runs for one general row
    /// of the type (#792).
    pub fn entity_part<P: EntityPart>(&mut self, entity: &str, part: P) {
        self.attach_entity_part(entity, type_name::<P>(), Arc::new(part));
    }
    /// [`Self::entity_part`] for a Part with typed state (`EntityPartState`): each entity
    /// of type `entity` keeps one `P::Data`, under `P::DATA`. Refused, and reported as the
    /// mod's registration refusal, when `P::DATA` syncs, copies on death, holds a resource
    /// handle, is outside the mod's namespaces, or names a key this mod already declared with
    /// another type. The entity type is checked once every type is known, when the game
    /// loads: only mob types (vanilla and mod) and mod general entity types keep Part state,
    /// and `minecraft:player`, any other vanilla family and an unknown id refuse the load,
    /// naming the type. `entity_part` itself checks no type.
    pub fn entity_part_with_state<P: crate::EntityPartState>(
        &mut self,
        entity: &str,
        part: P,
    ) -> Result<(), crate::EntityPartStateError> {
        self.declare_entity_part_state(
            entity,
            type_name::<P>(),
            crate::PartStateDeclaration::of(P::DATA),
        )?;
        self.entity_part(entity, part);
        Ok(())
    }
    /// Answer entity capability `key` for entity type `kind` (`mymod:robot`,
    /// `minecraft:player`) with `provider`, which gives a handler for one entity, or none:
    /// `r.entity_capability(&ENTITY_ITEMS, "mymod:robot", |_, robot, ()| Some(Box::new(Cargo(robot))))`.
    /// Asked as `block_entity_capability`'s are.
    pub fn entity_capability<T: ?Sized + 'static, C: 'static>(
        &mut self,
        key: &EntityCapabilityKey<T, C>,
        kind: &str,
        provider: impl Fn(&mut dyn ServerHost, EntityRef, C) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_entity_capability(
            key.name,
            kind,
            false,
            EntityProviderOf::of(Arc::new(provider)),
        );
    }
    /// `entity_capability`, asked ahead of the game's own provider,
    /// as a mod that replaces how a player's inventory is reached.
    pub fn entity_capability_before_game<T: ?Sized + 'static, C: 'static>(
        &mut self,
        key: &EntityCapabilityKey<T, C>,
        kind: &str,
        provider: impl Fn(&mut dyn ServerHost, EntityRef, C) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_entity_capability(
            key.name,
            kind,
            true,
            EntityProviderOf::of(Arc::new(provider)),
        );
    }
}

/// Registrar operations for entities.
pub trait EntitiesRegistrar {
    /// Attach `part`, of kind `kind`, to the entity type `entity`
    /// (`minecraft:zombie`, `mymod:golem`): its `register_goals` runs at every
    /// `Mob` construction of that type, and its `general_entity_tick` for one
    /// general row's authoritative tick (#792).
    fn attach_entity_part(&mut self, entity: &str, kind: &'static str, part: Arc<dyn EntityPart>);

    /// Record the state declaration of entity Part `part` on entity type `entity`, or refuse
    /// it (as the mod's registration refusal too): see `PartStateDeclaration::check`, a key
    /// outside this mod's namespaces, or one it declared with another Rust type name. Mods reach
    /// it through `entity_part_with_state`.
    fn declare_entity_part_state(
        &mut self,
        entity: &str,
        part: &'static str,
        declaration: crate::PartStateDeclaration,
    ) -> Result<(), crate::EntityPartStateError>;

    /// Answer entity capability `name` for entity type `kind` with `provider`; mods reach it
    /// through `entity_capability`.
    fn provide_entity_capability(
        &mut self,
        name: &'static str,
        kind: &str,
        before_game: bool,
        provider: EntityProviderOf,
    );

    /// Tell `entity_tick_pre` and `entity_tick_post` for each entity of type `entity`
    /// (`minecraft:zombie`, `mymod:golem`) alone, right before and right after its own tick
    /// (NeoForge's `EntityTickEvent` timing), instead of once with its family. Every mod that
    /// hooks them hears those entities alone. Name only the types that need their own moment:
    /// each entity of a named type costs a call of its own every tick. A type that is not a
    /// namespaced lowercase id refuses the mod's registration.
    fn entity_tick_each(&mut self, entity: &str);

    /// [`Self::entity_tick_each`] for every entity type the tick hooks observe,
    /// as a port of a NeoForge mod that listens to `EntityTickEvent` needs.
    fn entity_tick_each_entity(&mut self);
}

/// The entity types whose ticks are told one entity at a time, from every mod's
/// [`EntitiesRegistrar::entity_tick_each`], resolved when the mods are installed.
// hidden: the game reads it once at load to mark its entity types.
// no wire: resolved on the host from the registrar's `entity_tick_each` calls, which cross on
// their own; the set itself is never sent to a mod.
// exhaustive: its fields are private, so mods cannot build or destructure it
#[doc(hidden)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EntityTickEach {
    all: bool,
    /// Sorted, without repeats.
    kinds: Vec<String>,
}

impl EntityTickEach {
    /// The set the declarations name, in any order: `None` names every type.
    pub fn of(declarations: &[Option<String>]) -> Self {
        let all = declarations.iter().any(Option::is_none);
        let mut kinds: Vec<String> = if all {
            Vec::new()
        } else {
            declarations.iter().flatten().cloned().collect()
        };
        kinds.sort_unstable();
        kinds.dedup();
        Self { all, kinds }
    }
    /// Whether a mod named any type.
    pub fn any(&self) -> bool {
        self.all || !self.kinds.is_empty()
    }
    /// Whether a mod named every type.
    pub fn all(&self) -> bool {
        self.all
    }
    /// Whether entities of type `kind` are told one at a time.
    pub fn names(&self, kind: &str) -> bool {
        self.all
            || self
                .kinds
                .binary_search_by(|k| k.as_str().cmp(kind))
                .is_ok()
    }
}

/// The installed mods' [`EntityTickEach`], or `None` before the mods are installed, when no type
/// is named.
// hidden: the game reads it once at load.
#[doc(hidden)]
pub fn entity_tick_each() -> Option<&'static EntityTickEach> {
    installed().map(|parts| &parts.entity_tick_each)
}

/// CapabilityProviders operations for entities.
// hidden: per-kind lookup implementation, exposed for generated provider bindings
#[doc(hidden)]
pub trait EntitiesCapabilityProviders {
    /// The next enabled provider of entity capability `name` for entity type `kind`, advancing `index`.
    fn entity_provider(
        &self,
        name: &str,
        kind: &str,
        index: &mut u32,
    ) -> Option<(EntityProviderOf, &'static str, bool)>;
}

impl EntitiesCapabilityProviders for InstalledProviders {
    fn entity_provider(
        &self,
        name: &str,
        kind: &str,
        index: &mut u32,
    ) -> Option<(EntityProviderOf, &'static str, bool)> {
        next(&installed()?.keys.entity_capabilities, name, kind, index)
    }
}

#[cfg(test)]
mod callable_tests {
    use super::super::installation::{AttachedInput, InstalledInput, PartsInput};
    use super::*;
    use crate::{ItemKey, ItemStackView, ModEntry, PlayerRef, WorldEpoch};
    use std::cell::Cell;
    use std::num::NonZeroU32;

    struct Hooks;
    #[mod_api::__subscriptions]
    impl crate::Hooks for Hooks {}

    struct Unrelated;
    #[mod_api::sunrise_part]
    impl EntityPart for Unrelated {
        fn register_goals(&self, _: &mut crate::GoalRegistrar<'_>) {}
    }

    struct Panics;
    #[mod_api::sunrise_part]
    impl EntityPart for Panics {
        fn is_shearable(
            &self,
            _: &dyn Host,
            _: EntityRef,
            _: Option<PlayerRef>,
            _: ItemStackView<'_>,
        ) -> Option<bool> {
            panic!("test shearing callback panic");
        }
    }

    struct Answers(bool);
    #[mod_api::sunrise_part]
    impl EntityPart for Answers {
        fn is_shearable(
            &self,
            _: &dyn Host,
            _: EntityRef,
            _: Option<PlayerRef>,
            shears: ItemStackView<'_>,
        ) -> Option<bool> {
            assert_eq!((shears.count(), shears.damage()), (1, 8));
            Some(self.0)
        }
    }

    #[test]
    fn entity_callable_index_routes_only_matching_parts_and_skips_a_panicking_callback() {
        let entries = ["unrelated", "panic", "first", "later"];
        let mods: Vec<_> = entries
            .iter()
            .map(|&id| ModEntry::new(id, Box::new(Hooks)))
            .collect();
        crate::testing::with_parts_registration_probe(
            mods.into(),
            |_| (),
            |mods, ()| {
                crate::testing::with_dispatch(mods, || {
                    let at = |index: usize, part: Arc<dyn EntityPart>| AttachedInput {
                        mod_id: entries[index],
                        mod_index: index,
                        order: 0,
                        type_name: entries[index],
                        part,
                    };
                    crate::parts::install_input(InstalledInput {
                        keys: PartsInput {
                            entities: HashMap::from([(
                                "test:goat".into(),
                                vec![
                                    at(0, Arc::new(Unrelated)),
                                    at(1, Arc::new(Panics)),
                                    at(2, Arc::new(Answers(true))),
                                    at(3, Arc::new(Answers(false))),
                                ],
                            )]),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    assert!(
                        crate::dispatch_is_shearable::<()>("test:sheep", |_| panic!(
                            "prepared shears for another type"
                        ))
                        .is_none()
                    );
                    let host = crate::testing::FakeHost::new();
                    let attempts = Cell::new(0);
                    let ask = || {
                        crate::dispatch_is_shearable("test:goat", |mut parts| {
                            parts.find_map(|part| {
                                part.invoke(|part| {
                                    attempts.set(attempts.get() + 1);
                                    part.is_shearable(
                                        &host,
                                        EntityRef::new(WorldEpoch::new(1), 1),
                                        None,
                                        ItemStackView::resident(
                                            (ItemKey(1), NonZeroU32::MIN, Some(8)),
                                            &|_| panic!("scalar access projected the tool"),
                                        ),
                                    )
                                })
                                .flatten()
                            })
                        })
                    };
                    assert_eq!(ask(), Some(Some(true)));
                    assert_eq!(attempts.get(), 2);
                    assert_eq!(ask(), Some(Some(true)));
                    assert_eq!(attempts.get(), 3);
                    assert!(crate::runtime::poisoned(HookId::IsShearable, "panic"));
                });
            },
        );
    }
}
