//! Scoped view of a falling or bouncing entity and deferred landing actions (`Block.fallOn`,
//! `Block.bounceOn`).
use crate::{ServerHost, content::crash::current_native_mod};

/// Scoped live view of the entity landing on or bouncing off a block (`Block.fallOn`,
/// `Block.bounceOn`).
///
/// Reads reflect the entity at the landing or bounce call site (`Entity.position`,
/// `Entity.getDeltaMovement`, `Entity.fallDistance`, `Entity.onGround`, `Entity.isCrouching`,
/// `Entity.isSuppressingBounce`). Writes apply directly to the borrowed entity without
/// encoding a batch or crossing the whole level.
// exhaustive: only the engine constructs this view.
// no wire: an exclusive call-scoped borrow, not a payload.
pub struct LandingEntity<'a> {
    data: &'a mut dyn LandingEntityAccess,
}

impl<'a> LandingEntity<'a> {
    // hidden: the engine builds the view around the falling or bouncing entity.
    #[doc(hidden)]
    pub fn new(data: &'a mut dyn LandingEntityAccess) -> Self {
        Self { data }
    }

    /// The entity's runtime id (`Entity.getId`).
    pub fn id(&self) -> i32 {
        self.data.id()
    }

    /// The entity's registered type id (for example `"minecraft:player"`, `"minecraft:pig"`,
    /// or `"minecraft:item"`).
    pub fn kind(&self) -> &str {
        self.data.kind()
    }

    /// The entity's current position in blocks (`Entity.position`).
    pub fn position(&self) -> [f64; 3] {
        self.data.position()
    }

    /// The block containing the entity's feet (`Entity.blockPosition`).
    pub fn block_position(&self) -> [i32; 3] {
        let [x, y, z] = self.data.position();
        [x.floor() as i32, y.floor() as i32, z.floor() as i32]
    }

    /// The entity's current velocity in blocks per tick (`Entity.getDeltaMovement`).
    pub fn velocity(&self) -> [f64; 3] {
        self.data.velocity()
    }

    /// The entity's accumulated fall distance in blocks (`Entity.fallDistance`).
    pub fn fall_distance(&self) -> f64 {
        self.data.fall_distance()
    }

    /// Whether the entity is currently on the ground (`Entity.onGround`).
    pub fn on_ground(&self) -> bool {
        self.data.on_ground()
    }

    /// Whether the entity currently has the crouching pose (`Entity.isCrouching`).
    pub fn is_crouching(&self) -> bool {
        self.data.is_crouching()
    }

    /// Whether the entity is suppressing bounce (`Entity.isSuppressingBounce` /
    /// `Entity.isShiftKeyDown`, as checked by `SlimeBlock.fallOn` and `Entity.move`).
    pub fn is_suppressing_bounce(&self) -> bool {
        self.data.is_suppressing_bounce()
    }

    /// The entity's bounding-box width and height in blocks (`[getBbWidth(), getBbHeight()]`,
    /// as checked by `FarmlandBlock.fallOn`).
    pub fn dimensions(&self) -> [f32; 2] {
        self.data.dimensions()
    }

    /// Set the entity's velocity in blocks per tick (`Entity.setDeltaMovement`).
    /// Returns `false` without changing velocity if any component is non-finite.
    pub fn set_velocity(&mut self, velocity: [f64; 3]) -> bool {
        if !velocity.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.data.set_velocity(velocity)
    }

    /// Reset the entity's accumulated fall distance to zero (`Entity.resetFallDistance`).
    pub fn reset_fall_distance(&mut self) {
        self.data.reset_fall_distance();
    }
}

/// Engine-side access to a falling or bouncing entity.
// hidden: the engine owns entity access; listeners receive `LandingEntity`.
#[doc(hidden)]
pub trait LandingEntityAccess {
    fn id(&self) -> i32;
    fn kind(&self) -> &str;
    fn position(&self) -> [f64; 3];
    fn velocity(&self) -> [f64; 3];
    fn fall_distance(&self) -> f64;
    fn on_ground(&self) -> bool;
    fn is_crouching(&self) -> bool;
    fn is_suppressing_bounce(&self) -> bool;
    fn dimensions(&self) -> [f32; 2];
    fn set_velocity(&mut self, velocity: [f64; 3]) -> bool;
    fn reset_fall_distance(&mut self);
}

/// One owned native callback queued during `BlockPart::fall_on` or `BlockPart::bounce_on`
/// to run against `&mut dyn ServerHost` once the entity batch finishes.
// no wire: owns a process-local Rust callback closure.
// hidden: engine-only deferred landing queue entry.
// exhaustive: fields are private so only `LandingDeferred` creates entries.
#[doc(hidden)]
pub struct DeferredLandingAction {
    owner: Option<&'static str>,
    at_site: bool,
    action: Box<dyn FnOnce(&mut dyn ServerHost)>,
}

impl DeferredLandingAction {
    /// Construct a deferred landing callback for a synthetic/test caller.
    pub fn new(owner: Option<&'static str>, action: Box<dyn FnOnce(&mut dyn ServerHost)>) -> Self {
        Self {
            owner,
            action,
            at_site: false,
        }
    }

    /// Whether the caller must release the level loan and run this before resuming the fall.
    pub fn at_site(&self) -> bool {
        self.at_site
    }

    /// The owning mod id attributed when the callback was queued, if any.
    pub fn owner(&self) -> Option<&'static str> {
        self.owner
    }

    /// Run the deferred callback against the server host.
    pub fn run(self, host: &mut dyn ServerHost) {
        (self.action)(host);
    }
}

/// Engine staging for deferred native callbacks queued during one `fall_on` or `bounce_on`
/// dispatch.
// no wire: owns pending server-thread landing callbacks.
// hidden: the engine stages deferred callbacks here; listeners use `FallOnEvent::after_landing`
// and `BounceOnEvent::after_bounce`.
// exhaustive: its queue is private; only the engine constructs it.
#[doc(hidden)]
pub struct LandingDeferred<'a> {
    actions: &'a mut Vec<DeferredLandingAction>,
}

impl<'a> LandingDeferred<'a> {
    /// Wrap a local queue for one `fall_on` or `bounce_on` dispatch.
    pub fn new(actions: &'a mut Vec<DeferredLandingAction>) -> Self {
        Self { actions }
    }

    pub(crate) fn push(
        &mut self,
        at_site: bool,
        action: impl FnOnce(&mut dyn ServerHost) + 'static,
    ) {
        self.actions.push(DeferredLandingAction {
            owner: current_native_mod(),
            at_site,
            action: Box::new(action),
        });
    }

    /// Checkpoint queue length before invoking one listener under phase containment.
    pub(crate) fn checkpoint(&self) -> usize {
        self.actions.len()
    }

    /// Discard callbacks queued by a listener refused by the phase gate.
    pub(crate) fn rollback(&mut self, len: usize) {
        self.actions.truncate(len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Hooks, ModEntry, callback_owner::CallbackOwner, testing::FakeHost, testing::with_dispatch,
        world::phase::ModPhase, world::phase_refusal,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct StubEntity {
        id: i32,
        kind: &'static str,
        position: [f64; 3],
        velocity: [f64; 3],
        fall_distance: f64,
        on_ground: bool,
        crouching: bool,
        suppressing_bounce: bool,
        dimensions: [f32; 2],
    }

    impl LandingEntityAccess for StubEntity {
        fn id(&self) -> i32 {
            self.id
        }
        fn kind(&self) -> &str {
            self.kind
        }
        fn position(&self) -> [f64; 3] {
            self.position
        }
        fn velocity(&self) -> [f64; 3] {
            self.velocity
        }
        fn fall_distance(&self) -> f64 {
            self.fall_distance
        }
        fn on_ground(&self) -> bool {
            self.on_ground
        }
        fn is_crouching(&self) -> bool {
            self.crouching
        }
        fn is_suppressing_bounce(&self) -> bool {
            self.suppressing_bounce
        }
        fn dimensions(&self) -> [f32; 2] {
            self.dimensions
        }
        fn set_velocity(&mut self, velocity: [f64; 3]) -> bool {
            self.velocity = velocity;
            true
        }
        fn reset_fall_distance(&mut self) {
            self.fall_distance = 0.0;
        }
    }

    struct QuietMod;
    impl Hooks for QuietMod {}

    #[test]
    fn landing_entity_reads_and_validates_writes() {
        let mut stub = StubEntity {
            id: 9,
            kind: "minecraft:player",
            position: [-0.2, 64.0, -1.5],
            velocity: [0.0, -0.5, 0.0],
            fall_distance: 6.25,
            on_ground: true,
            crouching: true,
            suppressing_bounce: true,
            dimensions: [0.6, 1.5],
        };
        let mut view = LandingEntity::new(&mut stub);
        assert_eq!(view.id(), 9);
        assert_eq!(view.kind(), "minecraft:player");
        assert_eq!(view.position(), [-0.2, 64.0, -1.5]);
        assert_eq!(view.block_position(), [-1, 64, -2]);
        assert_eq!(view.velocity(), [0.0, -0.5, 0.0]);
        assert_eq!(view.fall_distance(), 6.25);
        assert!(view.on_ground());
        assert!(view.is_crouching());
        assert!(view.is_suppressing_bounce());
        assert_eq!(view.dimensions(), [0.6, 1.5]);

        assert!(!view.set_velocity([f64::NAN, 0.1, 0.0]));
        assert!(!view.set_velocity([0.0, f64::INFINITY, 0.0]));
        assert_eq!(view.velocity(), [0.0, -0.5, 0.0]);

        assert!(view.set_velocity([0.1, 0.8, -0.2]));
        assert_eq!(view.velocity(), [0.1, 0.8, -0.2]);
        view.reset_fall_distance();
        assert_eq!(view.fall_distance(), 0.0);
    }

    #[test]
    fn landing_deferred_checkpoints_and_rolls_back_tail_only() {
        let mut queue = Vec::new();
        let ran = Arc::new(AtomicUsize::new(0));
        {
            let mut deferred = LandingDeferred::new(&mut queue);
            let r1 = Arc::clone(&ran);
            deferred.push(false, move |_| {
                r1.fetch_add(1, Ordering::Relaxed);
            });
            let mark = deferred.checkpoint();
            let r2 = Arc::clone(&ran);
            deferred.push(true, move |_| {
                r2.fetch_add(10, Ordering::Relaxed);
            });
            deferred.rollback(mark);
        }
        assert_eq!(queue.len(), 1);
        let mut host = FakeHost::new();
        for action in queue {
            action.run(&mut host);
        }
        assert_eq!(ran.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn landing_callback_phase_refusal_does_not_disable_future_callbacks() {
        with_dispatch(
            vec![ModEntry::new("landing_refusal_mod", Box::new(QuietMod))].into(),
            || {
                let mut host = FakeHost::new();
                for _ in 0..2 {
                    let result = CallbackOwner::call_landing_callback(
                        "landing_refusal_mod",
                        &mut host,
                        |_| {
                            phase_refusal::refuse("landing_test_op", ModPhase::Setup);
                        },
                    );
                    assert!(result.is_err());
                }
                let ran = Arc::new(AtomicUsize::new(0));
                let r = Arc::clone(&ran);
                CallbackOwner::call_landing_callback("landing_refusal_mod", &mut host, |_| {
                    r.fetch_add(1, Ordering::Relaxed);
                })
                .expect("phase refusal must not disable future deferred landing callbacks");
                assert_eq!(ran.load(Ordering::Relaxed), 1);
            },
        );
    }
}
