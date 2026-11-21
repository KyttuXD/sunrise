use super::*;

type EffectTicks = HashMap<Box<str>, Vec<Attached<dyn MobEffectPart>>>;

// no wire: the load-owned table contains process-local trait objects.
// hidden: opaque resolution slot carried by the installation record.
/// Parts subscribed to effect ticks, frozen after effect keys resolve.
#[doc(hidden)]
#[derive(Default)]
#[non_exhaustive]
pub struct ResolvedEffectTicks(OnceLock<EffectTicks>);

pub(crate) fn resolved_effects() -> &'static OnceLock<sunrise_mod_runtime::registry_keys::Effects> {
    static EMPTY: OnceLock<sunrise_mod_runtime::registry_keys::Effects> = OnceLock::new();
    installed().map_or(&EMPTY, |installed| &installed.effects)
}

/// Registrar operations for effects.
pub trait EffectsRegistrar {
    /// Request an effect by registry ID. A missing effect in the complete registry refuses load.
    /// For optional integration, check
    /// [`RegistrationContext::has_optional_dependency`](crate::RegistrationContext::has_optional_dependency)
    /// before requesting the key.
    fn effect_key(&mut self, id: &str) -> crate::EffectKey;
    /// Attach tick behaviour to a registered mod effect regardless of its declaring package.
    /// A requested effect missing from the registry refuses loading. For optional integrations,
    /// check [`RegistrationContext::has_optional_dependency`](crate::RegistrationContext::has_optional_dependency)
    /// before requesting the key.
    fn attach_mob_effect_part(
        &mut self,
        effect: &crate::EffectKey,
        kind: &'static str,
        part: Arc<dyn MobEffectPart>,
    ) -> Result<(), MobEffectPartError>;
}

/// A refused effect Part registration.
#[derive(Clone, Debug, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
#[non_exhaustive]
pub enum MobEffectPartError {
    /// Vanilla effects keep vanilla tick behaviour.
    VanillaEffect {
        /// The vanilla effect that was requested.
        effect: crate::EffectKey,
    },
    /// This concrete Part type is already attached to the effect.
    Duplicate {
        /// The effect already carrying this Part type.
        effect: crate::EffectKey,
        /// The concrete Rust Part type name.
        part: String,
    },
    /// The effect key does not resolve to a requested registry slot.
    InvalidKey {
        /// The unresolved effect key.
        effect: crate::EffectKey,
    },
}

impl dyn crate::Registrar + '_ {
    /// Attach a Part to a registered mod effect, including one another mod declares later.
    /// The declaring package's effect Parts run first. Other Parts execute in dispatch order,
    /// then registration order. Each Part keeps its own cadence. Actual built-in effects
    /// keep their vanilla tick behaviour and refuse immediately.
    ///
    /// A missing target is a load error. Optional integrations should check
    /// [`RegistrationContext::has_optional_dependency`](crate::RegistrationContext::has_optional_dependency)
    /// for the package ID before requesting the key; the effect's namespace need not match it.
    pub fn mob_effect_part<P: MobEffectPart>(
        &mut self,
        effect: &crate::EffectKey,
        part: P,
    ) -> Result<(), MobEffectPartError> {
        self.attach_mob_effect_part(effect, type_name::<P>(), Arc::new(part))
    }
}

/// Freeze tick order after effect-key completion. The declaring package affects ordering only.
pub fn freeze_effect_part_order<'a>(owner: impl Fn(&str) -> Option<&'a str>) {
    let Some(installed) = installed() else {
        return;
    };
    installed.effect_ticks.0.get_or_init(|| {
        installed
            .keys
            .for_hook(HookId::ShouldApplyEffectTickThisTick)
            .map_or_else(EffectTicks::new, |table| {
                let mut ticks = table.effects.clone();
                for (id, parts) in &mut ticks {
                    let owner = owner(id);
                    parts.sort_by_key(|part| {
                        (Some(part.mod_id) != owner, part.mod_index, part.order)
                    });
                }
                ticks
            })
    });
}

/// Run each effect Part's cadence and action as one pair, sharing the holder snapshots.
/// Queued writes remain deferred until this entire dispatch returns.
pub fn dispatch_mob_effect_tick<'a>(
    host: &dyn Host,
    tick_count: i32,
    effect: crate::EffectKey,
    amplifier: i32,
    make_call: impl FnOnce(crate::EffectKey, i32) -> crate::__private::ApplyEffectTickCall<'a>,
) {
    let Some(ticks) = installed().and_then(|installed| installed.effect_ticks.0.get()) else {
        return;
    };
    let Some(attached) =
        crate::content::registry_keys::effect_id(effect).and_then(|id| ticks.get(id))
    else {
        return;
    };
    let mut due = crate::entered_parts(attached, "mob_effect_tick").filter(|part| {
        let mut should = crate::ShouldApplyEffectTickThisTickEvent {
            effect,
            tick_count,
            amplifier,
            apply: false,
        };
        part.should_apply_effect_tick_this_tick(host, &mut should);
        should.apply
    });
    let Some(first) = due.next() else {
        return;
    };
    let call = make_call(effect, amplifier);
    for part in std::iter::once(first).chain(due) {
        let mut event = crate::ApplyEffectTickEvent {
            effect: call.effect,
            dimension: call.dimension,
            entity: call.entity,
            amplifier: call.amplifier,
            holder: call.holder,
            health: call.health,
            max_health: call.max_health,
            food: call.food,
            active_effects: call.active_effects,
            changes: &mut *call.changes,
            keep: *call.keep,
        };
        part.apply_effect_tick(host, &mut event);
        *call.keep = event.keep;
        if !*call.keep {
            break;
        }
    }
}
