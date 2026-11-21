//! Savepoint coordinator for the remaining quick-move, workstation and merchant decisions.
//! A listener runs before the planned action; a refusal aborts that action's own writes.
use super::*;
use crate::game::transactions::decision::{
    self, ActionTarget, GameHost, Identity, InvalidReason, MenuId, Outcome, OwnerStamp, Reply,
    Side, StaleReason,
};
use mod_api::EntityRef;
use std::{cell::RefCell, rc::Rc};

/// The decision's menu owner and the lifecycle identity of its actor and target.
pub(in crate::game) struct DecidedTarget {
    pub(in crate::game) route: MenuId,
    pub(in crate::game) identity: Identity,
}

#[cfg(test)]
thread_local! {
    /// How many times `decided_action` was entered: the no-listener tests prove it stays zero.
    pub(in crate::game) static DECIDED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

type Fresh<'p> = Box<dyn Fn(&Game<'_>) -> bool + 'p>;
type Perform<'p> = Box<dyn FnOnce(&mut Game<'_>) + 'p>;

struct Shared<'p> {
    fresh: Fresh<'p>,
    perform: RefCell<Option<Perform<'p>>>,
}

#[derive(Clone)]
struct Action<'p> {
    id: MenuId,
    shared: Rc<Shared<'p>>,
}

struct Reads {
    player_writes: u64,
}

struct Plan;

impl<'a, 'g> ActionTarget<GameHost<'a, 'g>> for Action<'_> {
    type Reads = Reads;
    type View = Reads;
    type Plan = Plan;
    type Frozen = Plan;

    fn id(&self) -> MenuId {
        self.id
    }
    fn capture(&self, host: &GameHost<'a, 'g>) -> std::result::Result<Reads, Outcome> {
        Ok(Reads {
            player_writes: host.game.transactions.player_writes(),
        })
    }
    fn view(reads: &Reads) -> &Reads {
        reads
    }
    fn freeze(&self, _: &Reads, plan: Plan) -> std::result::Result<Plan, InvalidReason> {
        Ok(plan)
    }
    fn empty(_: &Plan) -> bool {
        false
    }
    fn validate(&self, host: &GameHost<'a, 'g>, reads: &Reads) -> std::result::Result<(), Outcome> {
        if host.game.transactions.player_writes() != reads.player_writes {
            return Err(Outcome::Stale(StaleReason::Mutation));
        }
        if (self.shared.fresh)(host.game) {
            Ok(())
        } else {
            Err(Outcome::Stale(StaleReason::Snapshot))
        }
    }
    fn apply(&self, host: &mut GameHost<'a, 'g>, _: Plan) {
        let perform = self.shared.perform.take().expect("an action runs once");
        perform(host.game);
    }
}

impl Game<'_> {
    /// Decide an action `name` on the menu `route` for the player entity `actor`: `fire` calls the
    /// hook and validates what it answered (`Reply::Allow`, or `Invalid` to refuse), `fresh` says
    /// the menu is still the one the action froze, and `perform` is the action, run once inside the
    /// savepoint when the decision stands. The result says whether it did.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::game) fn decided_action<'p>(
        &mut self,
        name: &'static str,
        route: MenuId,
        actor: EntityRef,
        fresh: impl Fn(&Game<'_>) -> bool + 'p,
        fire: impl FnOnce(&mut Game<'_>) -> Reply,
        perform: impl FnOnce(&mut Game<'_>) + 'p,
    ) -> bool {
        self.decided_action_on(
            name,
            DecidedTarget {
                route,
                identity: Identity {
                    side: Side::Server,
                    world: actor.epoch(),
                    actor,
                    target: actor,
                },
            },
            fresh,
            fire,
            perform,
        )
    }

    /// Run a decision whose action target is distinct from its acting player.
    pub(in crate::game) fn decided_action_on<'p>(
        &mut self,
        name: &'static str,
        target: DecidedTarget,
        fresh: impl Fn(&Game<'_>) -> bool + 'p,
        fire: impl FnOnce(&mut Game<'_>) -> Reply,
        perform: impl FnOnce(&mut Game<'_>) + 'p,
    ) -> bool {
        let DecidedTarget { route, identity } = target;
        #[cfg(test)]
        DECIDED.set(DECIDED.get() + 1);
        let target = Action {
            id: route,
            shared: Rc::new(Shared {
                fresh: Box::new(fresh),
                perform: RefCell::new(Some(Box::new(perform))),
            }),
        };
        // The action's savepoint needs a transaction to nest in: the open one, or this one.
        let seal = self.transaction_open();
        let outcome = {
            let mut host = GameHost {
                game: &mut *self,
                identity,
            };
            decision::run(&mut host, &target, |_, _| Plan, |host| fire(host.game))
        };
        self.transaction_close(&seal, matches!(outcome, Ok(Outcome::Applied)));
        match outcome {
            Ok(Outcome::Applied) => true,
            // A cancelled action is the listener's own answer, not a refusal: the caller goes on.
            Ok(Outcome::Canceled) => false,
            Ok(other) => {
                log::warn!("{name}: the action was refused ({other:?}); nothing changed");
                false
            }
            Err(admission) => {
                log::warn!("{name}: the action was not admitted ({admission:?}); nothing changed");
                false
            }
        }
    }

    /// The player entity of the guest `conn`, as the decision names it.
    pub(in crate::game) fn guest_actor(&self, conn: u32) -> Option<EntityRef> {
        let epoch = self.native_lifecycle.epoch()?;
        let entity = self.lan.as_ref()?.hub.guest(conn)?.entity_id;
        Some(EntityRef::new(epoch, u32::try_from(entity).ok()?))
    }

    /// The decision's menu id for the guest `conn`'s open workstation, and the owner's stamp: it
    /// advances on every access to the station, so a stamp that still matches says no one opened,
    /// closed or replaced it since it was read (also while a click has the menu lifted out).
    pub(in crate::game) fn guest_station(&self, conn: u32) -> Option<(MenuId, OwnerStamp)> {
        let owner = &self.lan.as_ref()?.menus.get(&conn)?.station;
        Some((owner.id(), owner.stamp()))
    }

    /// The decision's menu id for the guest `conn`'s hub menus.
    pub(in crate::game) fn guest_route(&mut self, conn: u32) -> Option<MenuId> {
        Some(self.lan.as_mut()?.menus.entry(conn).or_default().hub.id())
    }
}
