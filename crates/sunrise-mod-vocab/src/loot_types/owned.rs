//! Attribute every codec and bound-rule call to its registration's shared `CallbackOwner`.
use super::*;
use crate::callback_owner::CallbackOwner;
use std::sync::Arc;

pub(crate) fn function_codec(
    owner: CallbackOwner,
    codec: Arc<dyn ModLootFunctionCodec>,
) -> Arc<dyn ModLootFunctionCodec> {
    let conditional = owner.call("loot function type", "loot_function_metadata", || {
        codec.conditional()
    });
    Arc::new(FunctionCodec {
        owner,
        codec,
        conditional,
    })
}

pub(crate) fn condition_codec(
    owner: CallbackOwner,
    codec: Arc<dyn ModLootConditionCodec>,
) -> Arc<dyn ModLootConditionCodec> {
    Arc::new(ConditionCodec { owner, codec })
}

struct FunctionCodec {
    owner: CallbackOwner,
    codec: Arc<dyn ModLootFunctionCodec>,
    conditional: Result<bool, String>,
}
impl ModLootFunctionCodec for FunctionCodec {
    fn decode(&self, data: &serde_json::Value) -> Result<Arc<dyn ModLootFunction>, String> {
        self.conditional.as_ref().map_err(Clone::clone)?;
        let rule = self
            .owner
            .call("loot function type", "loot_function_decode", || {
                self.codec.decode(data)
            })?
            .map_err(|reason| format!("mod {}: {reason}", self.owner.id()))?;
        let params = match self
            .owner
            .call("loot function type", "loot_function_metadata", || {
                rule.referenced_context_params()
            }) {
            Ok(params) => params,
            Err(reason) => {
                self.owner
                    .dispose("loot function type", "loot_function_drop", rule);
                return Err(reason);
            }
        };
        Ok(Arc::new(OwnedFunction {
            owner: self.owner.clone(),
            rule: Some(rule),
            params,
        }))
    }
    fn conditional(&self) -> bool {
        self.conditional.as_ref().copied().unwrap_or(true)
    }
}

struct OwnedFunction {
    owner: CallbackOwner,
    rule: Option<Arc<dyn ModLootFunction>>,
    params: Vec<Identifier>,
}
impl Drop for OwnedFunction {
    fn drop(&mut self) {
        self.owner
            .dispose("loot function type", "loot_function_drop", self.rule.take());
    }
}
impl ModLootFunction for OwnedFunction {
    fn referenced_context_params(&self) -> Vec<Identifier> {
        self.params.clone()
    }
    fn apply(
        &self,
        stack: &mut LootStackEditor<'_>,
        context: &mut dyn LootTypeContext,
    ) -> Result<(), String> {
        self.owner
            .call("loot function type", "loot_function_apply", || {
                self.rule
                    .as_ref()
                    .expect("retained rule")
                    .apply(stack, context)
            })?
            .map_err(|reason| format!("mod {}: {reason}", self.owner.id()))
    }
}

struct ConditionCodec {
    owner: CallbackOwner,
    codec: Arc<dyn ModLootConditionCodec>,
}
impl ModLootConditionCodec for ConditionCodec {
    fn decode(&self, data: &serde_json::Value) -> Result<Arc<dyn ModLootCondition>, String> {
        let rule = self
            .owner
            .call("loot condition type", "loot_condition_decode", || {
                self.codec.decode(data)
            })?
            .map_err(|reason| format!("mod {}: {reason}", self.owner.id()))?;
        let params = match self
            .owner
            .call("loot condition type", "loot_condition_metadata", || {
                rule.referenced_context_params()
            }) {
            Ok(params) => params,
            Err(reason) => {
                self.owner
                    .dispose("loot condition type", "loot_condition_drop", rule);
                return Err(reason);
            }
        };
        Ok(Arc::new(OwnedCondition {
            owner: self.owner.clone(),
            rule: Some(rule),
            params,
        }))
    }
}

struct OwnedCondition {
    owner: CallbackOwner,
    rule: Option<Arc<dyn ModLootCondition>>,
    params: Vec<Identifier>,
}
impl Drop for OwnedCondition {
    fn drop(&mut self) {
        self.owner.dispose(
            "loot condition type",
            "loot_condition_drop",
            self.rule.take(),
        );
    }
}
impl ModLootCondition for OwnedCondition {
    fn referenced_context_params(&self) -> Vec<Identifier> {
        self.params.clone()
    }
    fn test(&self, context: &mut dyn LootTypeContext) -> Result<bool, String> {
        self.owner
            .call("loot condition type", "loot_condition_test", || {
                self.rule.as_ref().expect("retained rule").test(context)
            })?
            .map_err(|reason| format!("mod {}: {reason}", self.owner.id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct PanickingDrop(Arc<AtomicUsize>);
    impl ModLootCondition for PanickingDrop {
        fn test(&self, _: &mut dyn LootTypeContext) -> Result<bool, String> {
            Ok(true)
        }
    }
    impl Drop for PanickingDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("retired loot rule");
        }
    }

    #[test]
    fn retired_bound_rules_dispose_even_when_the_registration_is_disabled() {
        let owner = CallbackOwner::for_mod("retired_loot_condition");
        let dropped = Arc::new(AtomicUsize::new(0));
        for expected in 1..=2 {
            let prepared = OwnedCondition {
                owner: owner.clone(),
                rule: Some(Arc::new(PanickingDrop(dropped.clone()))),
                params: Vec::new(),
            };
            drop(prepared);
            assert_eq!(dropped.load(Ordering::Relaxed), expected);
            assert!(
                owner
                    .call("loot condition type", "loot_condition_test", || ())
                    .is_err()
            );
        }
    }
}
