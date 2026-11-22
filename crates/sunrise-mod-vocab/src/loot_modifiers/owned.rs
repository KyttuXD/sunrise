use super::*;
use crate::callback_owner::CallbackOwner;

pub(crate) fn owned_serializer(
    owner: CallbackOwner,
    serializer: Arc<dyn GlobalLootModifierSerializer>,
) -> Arc<dyn GlobalLootModifierSerializer> {
    Arc::new(OwnedSerializer { owner, serializer })
}

struct OwnedSerializer {
    owner: CallbackOwner,
    serializer: Arc<dyn GlobalLootModifierSerializer>,
}
impl GlobalLootModifierSerializer for OwnedSerializer {
    fn decode(
        &self,
        data: &Value,
        load: &dyn DataLoadContext,
    ) -> Result<Arc<dyn GlobalLootModifier>, LootModifierError> {
        let instance = self
            .owner
            .call(
                "loot modifier serializer",
                "global_loot_modifier_decode",
                || self.serializer.decode(data, load),
            )
            .map_err(|reason| LootModifierError::new("$", reason))??;
        // These resource methods execute mod code too. Capture them in
        // the same owning load and owner scope, never on the roll path.
        let metadata = self.owner.call(
            "loot modifier serializer",
            "global_loot_modifier_metadata",
            || -> Result<_, LootModifierError> {
                let priority = instance.priority()?;
                let condition = instance.condition()?;
                Ok((priority, condition))
            },
        );
        let (priority, condition) = match metadata {
            Ok(Ok(metadata)) => metadata,
            Ok(Err(error)) => {
                self.owner.dispose(
                    "loot modifier serializer",
                    "global_loot_modifier_drop",
                    instance,
                );
                return Err(error);
            }
            Err(reason) => {
                self.owner.dispose(
                    "loot modifier serializer",
                    "global_loot_modifier_drop",
                    instance,
                );
                return Err(LootModifierError::new("$", reason));
            }
        };
        Ok(Arc::new(OwnedModifier {
            owner: self.owner.clone(),
            instance,
            priority,
            condition,
        }))
    }
}

struct OwnedModifier {
    owner: CallbackOwner,
    instance: Arc<dyn GlobalLootModifier>,
    priority: i32,
    condition: Option<Value>,
}
impl GlobalLootModifier for OwnedModifier {
    fn priority(&self) -> Result<i32, LootModifierError> {
        Ok(self.priority)
    }
    fn condition(&self) -> Result<Option<Value>, LootModifierError> {
        Ok(self.condition.clone())
    }
    fn apply(
        &self,
        context: &mut dyn LootModifierContext,
        generated: &mut LootList<'_>,
    ) -> Result<(), LootModifierError> {
        self.owner
            .call("loot modifier", "global_loot_modifier_apply", || {
                self.instance.apply(context, generated)
            })
            .map_err(|reason| LootModifierError::new("apply", reason))?
    }
}
