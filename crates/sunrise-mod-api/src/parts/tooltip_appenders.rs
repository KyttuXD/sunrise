//! Freeze the component tooltip graph once, after all Parts register.
use super::*;
use crate::TooltipPlacement;
use std::{cmp::Reverse, collections::BinaryHeap};

pub(crate) struct TooltipAppenderRegistration {
    pub(crate) mod_id: &'static str,
    pub(crate) mod_index: usize,
    pub(crate) component: Box<str>,
    pub(crate) placement: TooltipPlacement,
    pub(crate) appender: Arc<dyn crate::TooltipAppender>,
}

/// `ItemTooltipHandler` and `CommonHooks.sortGraphChecked`: a chain of stock appenders
/// plus one edge per mod registration, with declaration order breaking topological ties.
pub(crate) fn freeze_tooltip_appenders(
    registrations: &[TooltipAppenderRegistration],
) -> Result<Vec<TooltipAppenderPart>, String> {
    let vanilla_count = VANILLA_TOOLTIP_APPENDERS.len();
    let count = vanilla_count + registrations.len();
    let mut indices = HashMap::with_capacity(count);
    for (index, &(_, id)) in VANILLA_TOOLTIP_APPENDERS.iter().enumerate() {
        indices.insert(id, index);
    }
    for (offset, registration) in registrations.iter().enumerate() {
        if let Some(existing) =
            indices.insert(registration.component.as_ref(), vanilla_count + offset)
        {
            let owner = if existing < vanilla_count {
                "vanilla"
            } else {
                registrations[existing - vanilla_count].mod_id
            };
            return Err(format!(
                "mod {}: duplicate tooltip appender for {}; already registered by {owner}",
                registration.mod_id, registration.component,
            ));
        }
    }

    let mut successors = vec![Vec::new(); count];
    let mut indegree = vec![0usize; count];
    let mut edges = std::collections::HashSet::new();
    let mut edge = |from: usize, to: usize| {
        if edges.insert((from, to)) {
            successors[from].push(to);
            indegree[to] += 1;
        }
    };
    for index in 1..vanilla_count {
        edge(index - 1, index);
    }
    for (offset, registration) in registrations.iter().enumerate() {
        let index = vanilla_count + offset;
        let (anchor, before) = match &registration.placement {
            TooltipPlacement::BeforeAll => (0, true),
            TooltipPlacement::AfterAll => (vanilla_count - 1, false),
            TooltipPlacement::Before(id) | TooltipPlacement::After(id) => {
                let Some(&anchor) = indices.get(id.as_str()) else {
                    return Err(format!(
                        "mod {}: tooltip appender for {} has no appender to anchor to at {id}",
                        registration.mod_id, registration.component,
                    ));
                };
                if anchor == index {
                    return Err(format!(
                        "mod {}: tooltip appender for {id} cannot anchor to itself",
                        registration.mod_id,
                    ));
                }
                (
                    anchor,
                    matches!(&registration.placement, TooltipPlacement::Before(_)),
                )
            }
        };
        if before {
            edge(index, anchor);
        } else {
            edge(anchor, index);
        }
    }

    let mut ready: BinaryHeap<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, &degree)| (degree == 0).then_some(Reverse(index)))
        .collect();
    let mut resolved = Vec::with_capacity(registrations.len());
    let mut slot = 0;
    while let Some(Reverse(index)) = ready.pop() {
        if index < vanilla_count {
            slot += 1;
        } else {
            let registration = &registrations[index - vanilla_count];
            resolved.push(TooltipAppenderPart {
                mod_id: registration.mod_id,
                mod_index: registration.mod_index,
                component: registration.component.clone(),
                slot,
                appender: registration.appender.clone(),
                panicked: Default::default(),
            });
        }
        for &next in &successors[index] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.push(Reverse(next));
            }
        }
    }
    if indegree.iter().any(|&degree| degree != 0) {
        let blocked: Vec<_> = registrations
            .iter()
            .zip(&indegree[vanilla_count..])
            .filter(|&(_, degree)| *degree != 0)
            .map(|(registration, _)| {
                format!("{} ({})", registration.component, registration.mod_id)
            })
            .collect();
        return Err(format!(
            "component tooltip appender ordering cycle blocks {}",
            blocked.join(", "),
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientHost, ItemStack, Text, TooltipAppendContext, TooltipAppender};

    struct Appender;
    impl TooltipAppender for Appender {
        fn append(
            &self,
            _: &dyn ClientHost,
            _: &ItemStack,
            _: &TooltipAppendContext,
            _: &mut Vec<(Text, u32)>,
        ) {
        }
    }

    fn registration(
        mod_id: &'static str,
        component: &str,
        placement: TooltipPlacement,
    ) -> TooltipAppenderRegistration {
        TooltipAppenderRegistration {
            mod_id,
            mod_index: 0,
            component: component.into(),
            placement,
            appender: Arc::new(Appender),
        }
    }

    #[test]
    fn tooltip_graph_resolves_forward_native_component_anchors() {
        let registrations = [
            registration(
                "reader",
                "reader:charge",
                TooltipPlacement::After("source:heat".into()),
            ),
            registration(
                "source",
                "source:heat",
                TooltipPlacement::Before("reader:charge".into()),
            ),
        ];
        // The two placements declare the same edge. Neither ties the pair to a stock
        // entry, so the first ready stock entry retains priority until the chain ends.
        let resolved = freeze_tooltip_appenders(&registrations).unwrap();
        assert_eq!(
            resolved
                .iter()
                .map(|part| (part.component.as_ref(), part.mod_id))
                .collect::<Vec<_>>(),
            [("source:heat", "source"), ("reader:charge", "reader")],
        );
        for part in resolved {
            assert_eq!(
                VANILLA_TOOLTIP_APPENDERS[part.slot - 1].1,
                "minecraft:damage"
            );
            assert!(VANILLA_TOOLTIP_APPENDERS.get(part.slot).is_none());
        }
    }

    #[test]
    fn tooltip_graph_rejects_missing_appender_anchors() {
        // Repair cost is a vanilla component type, but it has no vanilla appender.
        let registrations = [registration(
            "maker",
            "maker:charge",
            TooltipPlacement::Before("minecraft:repair_cost".into()),
        )];
        assert_eq!(
            freeze_tooltip_appenders(&registrations).err().as_deref(),
            Some(
                "mod maker: tooltip appender for maker:charge has no appender to anchor to at minecraft:repair_cost"
            ),
        );
    }

    #[test]
    fn tooltip_graph_rejects_duplicate_vanilla_or_mod_appender_ownership() {
        let vanilla = [registration(
            "maker",
            "minecraft:damage",
            TooltipPlacement::AfterAll,
        )];
        assert_eq!(
            freeze_tooltip_appenders(&vanilla).err().as_deref(),
            Some(
                "mod maker: duplicate tooltip appender for minecraft:damage; already registered by vanilla"
            ),
        );
        let modded = [
            registration("maker", "maker:charge", TooltipPlacement::AfterAll),
            registration("other", "maker:charge", TooltipPlacement::BeforeAll),
        ];
        assert_eq!(
            freeze_tooltip_appenders(&modded).err().as_deref(),
            Some(
                "mod other: duplicate tooltip appender for maker:charge; already registered by maker"
            ),
        );
    }

    #[test]
    fn tooltip_graph_rejects_cycles_between_mod_appenders() {
        let registrations = [
            registration(
                "maker",
                "maker:charge",
                TooltipPlacement::Before("other:glow".into()),
            ),
            registration(
                "other",
                "other:glow",
                TooltipPlacement::Before("maker:charge".into()),
            ),
        ];
        assert_eq!(
            freeze_tooltip_appenders(&registrations).err().as_deref(),
            Some(
                "component tooltip appender ordering cycle blocks maker:charge (maker), other:glow (other)"
            ),
        );
    }
}
