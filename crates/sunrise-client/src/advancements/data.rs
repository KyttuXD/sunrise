//! Advancement file codecs; reject only the owning malformed file.
use super::*;

pub(super) struct PredicateSource<'a, 'o> {
    predicates: Option<&'a HashMap<String, Value>>,
    loaded_predicates: Option<HashMap<String, Value>>,
    captured_types: Option<sunrise_world::loot::types::Types>,
    data: Option<&'a Path>,
    overlay: Option<&'a sunrise_world::loot::PackOverlay<'o>>,
}

impl<'a, 'o> PredicateSource<'a, 'o> {
    pub(super) fn from_disk(
        data: &'a Path,
        overlay: &'a sunrise_world::loot::PackOverlay<'o>,
    ) -> Self {
        Self {
            predicates: None,
            loaded_predicates: None,
            captured_types: None,
            data: Some(data),
            overlay: Some(overlay),
        }
    }

    pub(super) fn from_predicates(predicates: &'a HashMap<String, Value>) -> Self {
        Self {
            predicates: Some(predicates),
            loaded_predicates: None,
            captured_types: None,
            data: None,
            overlay: None,
        }
    }

    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            predicates: None,
            loaded_predicates: Some(HashMap::new()),
            captured_types: None,
            data: None,
            overlay: None,
        }
    }

    fn resolve(&mut self) -> (&HashMap<String, Value>, &sunrise_world::loot::types::Types) {
        let predicates = match self.predicates {
            Some(predicates) => predicates,
            None => self
                .loaded_predicates
                .get_or_insert_with(|| match (self.data, self.overlay) {
                    (Some(data), Some(overlay)) => load_data_predicates(data, overlay),
                    _ => HashMap::new(),
                }),
        };
        let types = self
            .captured_types
            .get_or_insert_with(sunrise_world::loot::types::Types::capture);
        (predicates, types)
    }
}

fn load_data_predicates(
    data: &Path,
    overlay: &sunrise_world::loot::PackOverlay<'_>,
) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Ok(namespaces) = std::fs::read_dir(data) {
        for ns in namespaces.flatten() {
            let Ok(folder) = sunrise_world::json_folder::read(&ns.path().join("predicate")) else {
                continue;
            };
            let ns = ns.file_name().to_string_lossy().into_owned();
            for file in &folder.files {
                let id = format!("{ns}:{}", file.name);
                if let Ok(v) = serde_json::from_slice::<Value>(file.bytes().unwrap_or_default()) {
                    out.insert(id, v);
                }
            }
        }
    }
    for (id, files) in overlay.kind("predicate") {
        match serde_json::from_slice::<Value>(sunrise_world::loot::PackOverlay::top(files)) {
            Ok(v) => {
                out.insert(id.to_owned(), v);
            }
            Err(e) => {
                out.remove(id);
                log::warn!("Sunrise: pack predicate {id} not loaded: {e}");
            }
        }
    }
    out.retain(|id, v| overlay.conditions.allows_id(v, "predicate", id));
    out
}

#[cfg(test)]
pub(super) fn parse(id: &str, v: &Value) -> Option<Advancement> {
    parse_with(id, v, &mut PredicateSource::empty())
}

pub(super) fn parse_with(
    id: &str,
    v: &Value,
    source: &mut PredicateSource<'_, '_>,
) -> Option<Advancement> {
    if let Err(error) = sunrise_world::item_defaults::validate_stack(&v["display"]["icon"])
        .and_then(|()| sunrise_world::item_defaults::validate_text(&v["display"]["title"]))
        .and_then(|()| sunrise_world::item_defaults::validate_text(&v["display"]["description"]))
    {
        log::warn!("advancement {id} rejected: {error}");
        return None;
    }
    let mut criteria = Vec::new();
    for (name, criterion) in v["criteria"].as_object()? {
        let Some(trigger) = criterion["trigger"]
            .as_str()
            .and_then(|key| sunrise_core::identifier::Identifier::parse(key).ok())
            .filter(|key| !key.path().is_empty())
        else {
            log::warn!("advancement {id} rejected: criterion {name} has an invalid trigger id");
            return None;
        };
        let builtin = is_builtin_trigger(trigger.as_str());
        let conditions = &criterion["conditions"];
        let player = if builtin {
            validate_builtin_conditions(conditions).map(|()| None)
        } else if conditions.is_null() || conditions.is_object() {
            // SimpleInstance owns only `player`. The trigger's other fields are opaque to the game.
            let player = &conditions["player"];
            if player.is_null() {
                Ok(None)
            } else {
                let (predicates, loot_types) = source.resolve();
                loot_types
                    .validate_condition_field(player, predicates, "conditions.player")
                    .map_err(|error| error.to_string())
                    .and_then(|()| bind_condition(player, predicates))
                    .map(Some)
            }
        } else {
            Err("conditions: expected an object".into())
        };
        let player = match player {
            Ok(player) => player,
            Err(error) => {
                log::warn!("advancement {id} rejected: criterion {name}: {error}");
                return None;
            }
        };
        criteria.push(Criterion {
            name: name.clone(),
            trigger: trigger.as_str().to_owned(),
            conditions: if builtin {
                LazyJson::new(conditions)
            } else {
                conditions.clone().into()
            },
            player,
        });
    }
    let requirements = match v["requirements"].as_array() {
        Some(r) => r
            .iter()
            .map(|group| {
                group
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .collect(),
        // AdvancementRequirements.allOf: every criterion.
        None => criteria.iter().map(|c| vec![c.name.clone()]).collect(),
    };
    let d = &v["display"];
    let display = d.is_object().then(|| Display {
        icon: crate::crafting::recipe_result(&d["icon"])
            .unwrap_or_else(|| crate::items::ItemStack::new("minecraft:air", 1)),
        title: d["title"].clone(),
        description: d["description"].clone(),
        frame: frame(&d["frame"]),
        background: d["background"].as_str().map(str::to_owned),
        show_toast: d["show_toast"].as_bool().unwrap_or(true),
        announce: d["announce_to_chat"].as_bool().unwrap_or(true),
        hidden: d["hidden"].as_bool().unwrap_or(false),
    });
    let r = &v["rewards"];
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect()
    };
    Some(Advancement {
        id: id.to_owned(),
        parent: v["parent"].as_str().map(|p| full(p).into_owned()),
        display,
        criteria,
        requirements,
        rewards: Rewards {
            recipes: strings(&r["recipes"]),
            experience: r["experience"].as_i64().unwrap_or(0) as i32,
            loot: strings(&r["loot"]),
            function: r["function"].as_str().map(|id| full(id).into_owned()),
        },
    })
}

fn validate_builtin_conditions(c: &Value) -> Result<(), String> {
    use sunrise_world::loot::{
        validate_entity_predicate, validate_item_predicate, validate_loot_condition,
    };
    validate_loot_condition(&c["player"])?;
    for key in ["item", "rod", "fired_from_weapon"] {
        validate_item_predicate(&c[key])?;
    }
    for key in ["items", "ingredients"] {
        for item in c[key].as_array().into_iter().flatten() {
            validate_item_predicate(item)?;
        }
    }
    for key in [
        "bystander",
        "cause",
        "child",
        "entity",
        "lightning",
        "location",
        "parent",
        "partner",
        "projectile",
        "source",
        "villager",
        "zombie",
        "victims",
    ] {
        validate_loot_condition(&c[key])?;
    }
    for damage in [&c["killing_blow"], &c["damage"]["type"]] {
        validate_entity_predicate(&damage["direct_entity"])?;
        validate_entity_predicate(&damage["source_entity"])?;
    }
    validate_loot_condition(&c["damage"]["source_entity"])?;
    for location in [&c["start_position"], &c["location"]] {
        validate_item_predicate(&location["block"])?;
    }
    Ok(())
}

/// Bind a validated `LootItemCondition.CODEC` value from its `"type"` discriminator
/// and active predicate registry, matching `RegistryFileCodec.decode`.
fn bind_condition(
    v: &Value,
    predicates: &HashMap<String, Value>,
) -> Result<BoundCondition, String> {
    if let Some(id) = v.as_str() {
        let id = sunrise_core::identifier::Identifier::parse(id)
            .map_err(|reason| reason.to_string())?
            .to_string();
        let target = predicates
            .get(&id)
            .ok_or_else(|| format!("unknown predicate reference {id}"))?;
        return bind_condition(target, predicates);
    }
    let kind = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "expected typed loot condition".to_owned())?;
    let kind = sunrise_core::identifier::Identifier::parse(kind)
        .map_err(|reason| reason.to_string())?
        .to_string();
    match kind.as_str() {
        "minecraft:inverted" => Ok(BoundCondition::Inverted(Box::new(bind_condition(
            &v["term"], predicates,
        )?))),
        "minecraft:all_of" => Ok(BoundCondition::AllOf(bind_condition_set(
            &v["terms"],
            predicates,
        )?)),
        "minecraft:any_of" => Ok(BoundCondition::AnyOf(bind_condition_set(
            &v["terms"],
            predicates,
        )?)),
        _ => Ok(BoundCondition::Leaf {
            kind,
            data: v.clone(),
        }),
    }
}

fn bind_condition_set(
    v: &Value,
    predicates: &HashMap<String, Value>,
) -> Result<Vec<BoundCondition>, String> {
    match v {
        Value::Array(list) => list
            .iter()
            .map(|term| bind_condition(term, predicates))
            .collect(),
        other => Ok(vec![bind_condition(other, predicates)?]),
    }
}
