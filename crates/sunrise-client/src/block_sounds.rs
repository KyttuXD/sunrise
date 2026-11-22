//! A block state's sound type as its parts answer (`IBlockExtension.getSoundType`): what the game
//! reads to play a block's step, fall, place or break sound, or a sniffer's dig.
use crate::blocks::BlockFacts;
use crate::mobs::{Mob, MobWorld};
use mod_api::{BlockSoundCause, BlockSoundType};
use std::borrow::Cow;
use sunrise_core::position::BlockPos;
use sunrise_world::{native_load_context::LoopContext, registry::BlockStateId};

/// Ask subscribing parts for the sound type of `state` at `pos` (`IBlockExtension.getSoundType`).
/// `own` is the state's already-resolved sound type. `None` means no part answered and the
/// caller can keep borrowing `own`. The caller reads the sound needed for `cause`.
/// `entity` is the stepping, landing, placing or digging entity's id and type, or `None` for a
/// break or a placement by no entity.
#[inline(never)]
pub fn native_sound_type(
    context: &LoopContext,
    pos: BlockPos,
    state: BlockStateId,
    entity: Option<(u32, &str)>,
    cause: BlockSoundCause,
    own: &BlockSoundType,
) -> Option<BlockSoundType> {
    crate::native_client_hooks::sound_type(context, pos, state, entity, cause, own)
}

/// The sound type of `state` at `pos` for `cause`: the state's own (`facts`), through
/// [`native_sound_type`].
pub fn sound_type<'a>(
    context: &LoopContext,
    facts: &'a BlockFacts,
    pos: BlockPos,
    state: BlockStateId,
    entity: Option<(u32, &str)>,
    cause: BlockSoundCause,
) -> Cow<'a, BlockSoundType> {
    let own = &facts.get(state).sounds;
    match native_sound_type(context, pos, state, entity, cause, own) {
        Some(answer) => Cow::Owned(answer),
        None => Cow::Borrowed(own),
    }
}

impl MobWorld<'_> {
    /// The sound type of `state` at `pos` that the mob `m` steps or lands on (`cause`), as the
    /// block's parts answer.
    pub(crate) fn block_sound_type(
        &self,
        m: &Mob,
        [x, y, z]: [i32; 3],
        state: BlockStateId,
        cause: BlockSoundCause,
    ) -> Cow<'_, BlockSoundType> {
        let (pos, entity) = (BlockPos::new(x, y, z), Some((m.id, m.kind.id.as_str())));
        sound_type(self.loop_context, self.facts, pos, state, entity, cause)
    }
}
