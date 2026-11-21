//! Layers on living entity renderers, added by mods on the client: `native_register_entity_layers`
//! (NeoForge's `EntityRenderersEvent.AddLayers`) is asked on every resource load, after the entity
//! renderers resolve, and the layers it collects are resolved to the slot each draw reads: a mob's
//! type (its shared `MobType`, which the game's types never replace while it runs) or a player's
//! model. A frame then hands an entity to its own target's layers, after its model and the vanilla
//! layers, each through the guard renderers draw through ([`Game::guarded_entity_draw`]). With no
//! layer at all, a frame checks one flag per entity and builds nothing.
use super::{
    Game,
    entity_type_rows::{EntityTypeRows, PLAYER},
    native_entity_renderers::{EntityDrawCallback, RenderersByType},
    native_mob_draw::{entity_render_state, mob_pose},
};
use glam::DVec3;
use mod_api::{
    __private::game::{CallbackOwner, RegistrationContextNew},
    EntityLayers, EntityRenderLayer, FrameModel, LayerTarget, PlayerModel, RegistrationContext,
    content::crash::current_native_mod,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex, PoisonError, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use sunrise_client::{
    fast_hash::FxHashMap,
    mesher::Vertex,
    mobs::{Mob, MobType},
};

const PLAYER_MODELS: &[PlayerModel] = &[PlayerModel::Wide, PlayerModel::Slim];

type Layer = EntityDrawCallback<dyn EntityRenderLayer>;

/// One target's layers, in the order they were added.
pub(in crate::game) type Layers = Arc<[EntityDrawCallback<dyn EntityRenderLayer>]>;

/// A layer re-added from the same Arc is still the same callback. The weak reference keeps its
/// address from being reused without keeping the layer alive after a resource reload replaces it.
#[derive(Default)]
pub(super) struct LayerOwners(Mutex<BTreeMap<(&'static str, usize), LayerOwner>>);

struct LayerOwner {
    layer: Weak<dyn EntityRenderLayer>,
    owner: CallbackOwner,
}

static OWNERS: LayerOwners = LayerOwners(Mutex::new(BTreeMap::new()));

/// Whether the last load resolved any layer: a frame checks only this when none was added.
static ANY: AtomicBool = AtomicBool::new(false);

/// The table the last resource load resolved; none before the first.
static TABLE: RwLock<Option<LayerTable>> = RwLock::new(None);

/// What one `register_entity_layers` round collects: the [`EntityLayers`] mods are handed, with
/// what this load's renderers are, and the layers added to each target.
pub(super) struct LayerRegistration<'a> {
    rows: &'a EntityTypeRows,
    mod_renderers: &'a RenderersByType,
    /// Whether a living type draws where layers run (as a mob).
    routed: &'a dyn Fn(&str) -> bool,
    owners: &'a LayerOwners,
    entity: BTreeMap<String, Vec<Layer>>,
    wide: Vec<Layer>,
    slim: Vec<Layer>,
}

impl<'a> LayerRegistration<'a> {
    pub(super) fn new(
        rows: &'a EntityTypeRows,
        mod_renderers: &'a RenderersByType,
        routed: &'a dyn Fn(&str) -> bool,
        owners: &'a LayerOwners,
    ) -> Self {
        Self {
            rows,
            mod_renderers,
            routed,
            owners,
            entity: BTreeMap::new(),
            wide: Vec::new(),
            slim: Vec::new(),
        }
    }

    /// Why `id` can't take a layer; none for a living entity type.
    fn refusal(&self, id: &str) -> Option<&'static str> {
        if id == PLAYER {
            Some("a player takes layers through its model (wide or slim)")
        } else if !self.rows.contains(id) {
            Some("it is no entity type")
        } else if !self.rows.is_living(id) {
            Some("its renderer is not a living entity's, and only those take layers")
        } else if !(self.routed)(id) {
            Some("it draws through a route of its own, which does not run layers yet")
        } else {
            None
        }
    }
}

impl EntityLayers for LayerRegistration<'_> {
    fn entity_types(&self) -> &[String] {
        &self.rows.non_player_ids
    }

    fn is_living(&self, entity_type: &str) -> bool {
        entity_type != PLAYER && self.rows.is_living(entity_type)
    }

    fn player_models(&self) -> &[PlayerModel] {
        PLAYER_MODELS
    }

    fn has_mod_renderer(&self, entity_type: &str) -> bool {
        self.rows.contains(entity_type) && self.mod_renderers.contains_key(entity_type)
    }

    fn add(
        &mut self,
        target: LayerTarget,
        layer: Arc<dyn EntityRenderLayer>,
    ) -> Result<(), String> {
        let Some(mod_id) = current_native_mod() else {
            let why = format!("an entity layer for {target:?} was added outside a mod callback");
            log::warn!("{why}");
            return Err(why);
        };
        let layers = match target {
            LayerTarget::Player(PlayerModel::Wide) => &mut self.wide,
            LayerTarget::Player(PlayerModel::Slim) => &mut self.slim,
            LayerTarget::Entity(id) => {
                if let Some(why) = self.refusal(&id) {
                    let why = format!("the entity layer for {id} is refused: {why}");
                    log::warn!("Mod {mod_id}: {why}");
                    return Err(why);
                }
                self.entity.entry(id).or_default()
            }
        };
        let key = (mod_id, Arc::as_ptr(&layer).cast::<()>() as usize);
        let owner = self
            .owners
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_insert_with(|| LayerOwner {
                layer: Arc::downgrade(&layer),
                owner: CallbackOwner::for_mod(mod_id),
            })
            .owner
            .clone();
        layers.push(EntityDrawCallback {
            inner: layer,
            owner,
        });
        Ok(())
    }
}

/// The layers by the slot each draw reads: rebuilt on every resource load.
#[derive(Default)]
pub(super) struct LayerTable {
    /// By the address of the mob type the game shares with every mob of the type.
    mobs: FxHashMap<usize, Layers>,
    wide: Option<Layers>,
    slim: Option<Layers>,
}

impl LayerTable {
    /// The table for what `registration` collected, with `mob_types` the types the game draws as
    /// mobs (the only living types `add` accepts).
    pub(super) fn resolve(
        registration: LayerRegistration<'_>,
        mob_types: &HashMap<String, Arc<MobType>>,
    ) -> Self {
        let resolved =
            |layers: Vec<Layer>| -> Option<Layers> { (!layers.is_empty()).then(|| layers.into()) };
        let mobs = registration
            .entity
            .into_iter()
            .filter_map(|(id, layers)| {
                let kind = mob_types.get(&id)?;
                Some((Arc::as_ptr(kind) as usize, resolved(layers)?))
            })
            .collect();
        Self {
            mobs,
            wide: resolved(registration.wide),
            slim: resolved(registration.slim),
        }
    }

    fn is_empty(&self) -> bool {
        self.mobs.is_empty() && self.wide.is_none() && self.slim.is_none()
    }

    /// The layers of `mob`: its player model's for a player, else its type's.
    pub(super) fn of(&self, mob: &Mob) -> Option<&Layers> {
        match (&*mob.kind.id == PLAYER, mob.slim) {
            (true, true) => self.slim.as_ref(),
            (true, false) => self.wide.as_ref(),
            (false, _) => self.mobs.get(&(Arc::as_ptr(&mob.kind) as usize)),
        }
    }
}

impl Game<'_> {
    /// The layers the last load resolved for `mob` (a player's model's for a player), looked up
    /// once per mob a frame: the answer both decides whether the draw keeps the mob's final model
    /// and is what `mod_layer_geometry` draws. With no layer added at all it is none after one
    /// flag check, with no lock and no lookup.
    pub(in crate::game) fn mod_layers(&self, mob: &Mob) -> Option<Layers> {
        if !ANY.load(Ordering::Relaxed) {
            return None;
        }
        let table = TABLE.read().unwrap_or_else(PoisonError::into_inner);
        table.as_ref()?.of(mob).cloned()
    }

    /// Asks the mods for their layers on the renderers this load built (`rows` its entity types,
    /// `mod_renderers` the registered renderers) and resolves them.
    pub(super) fn resolve_entity_layers(
        &self,
        rows: &EntityTypeRows,
        mod_renderers: &RenderersByType,
    ) {
        let mob_types = self.res.mob_types();
        let routed = |id: &str| mob_types.contains_key(id);
        let mut registration = LayerRegistration::new(rows, mod_renderers, &routed, &OWNERS);
        Self::native_register_entity_layers(&RegistrationContext::new(), &mut registration);
        let table = LayerTable::resolve(registration, mob_types);
        let any = !table.is_empty();
        let previous = TABLE
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(table);
        ANY.store(any, Ordering::Relaxed);
        drop(previous);
        OWNERS
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, entry| entry.layer.strong_count() != 0);
    }

    /// Draws `layers` over `mob`, told as the entity `id`, each from its frame (at the feet,
    /// turned with the body), in the order they were added. Each layer borrows the same final
    /// `model`, if this draw produced a body pose.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::game) fn registered_layer_geometry(
        &self,
        mob: &Mob,
        id: u32,
        layers: &Layers,
        model: Option<&dyn FrameModel>,
        partial: f32,
        camera: DVec3,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        let entity = entity_render_state(mob, id, partial);
        let feet = mob.previous.lerp(mob.position, f64::from(partial));
        let feet = DVec3::new(feet.x, feet.y, feet.z);
        let pose = mob_pose(
            feet - camera,
            entity.living.expect("mob living state").body_rot,
        );
        let block = [feet.x, feet.y, feet.z].map(|c| c.floor() as i32);
        for layer in layers.iter() {
            self.guarded_entity_draw(
                &layer.owner,
                "entity layer",
                partial,
                &mob.kind.id,
                block,
                pose,
                light,
                None,
                out,
                |draw| layer.inner.submit(&entity, model, draw),
            );
        }
    }
}
