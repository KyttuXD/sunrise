//! Owner enablement, attribution and panic containment for retained resources.
use crate::world::phase_refusal::{self, PhaseRefusal, contain_refusal};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// A retained mod callback panicked and was disabled for the rest of this session.
// no wire: in-process notification drained by the client's error toast queue.
// exhaustive: engine-only notification record produced by `callback_owner::call`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackPanicNotice {
    /// The `[mod] id` whose callback panicked.
    pub mod_id: &'static str,
    /// Callback category (for example `"menu screen"` or `"debug entry"`).
    pub kind: &'static str,
    /// Specific callback operation (for example `"menu_screen_draw"`).
    pub operation: &'static str,
    /// Panic message payload, if it was a string.
    pub message: String,
}

static PANIC_NOTICES: Mutex<VecDeque<CallbackPanicNotice>> = Mutex::new(VecDeque::new());

thread_local! {
    static CONTAINED: Cell<u32> = const { Cell::new(0) };
    static CAUGHT: RefCell<Option<String>> = const { RefCell::new(None) };
    static RETAINED_CALLBACK: Cell<bool> = const { Cell::new(false) };
    // Keep the original fatal payload in the engine-owned invocation, not in the
    // unwind a wrapper is free to catch. It is taken on exit from the outermost wrapper.
    static DELEGATED_ENGINE_PANIC: RefCell<Option<Box<dyn Any + Send>>> = const { RefCell::new(None) };
}

// The boundary may be monomorphized outside mod_vocab. Keep the thread-local reads and
// writes in non-inline functions so the callback and the game hook share the same slot.
struct PanicScope(Option<String>);

impl PanicScope {
    #[inline(never)]
    fn enter() -> Self {
        CONTAINED.with(|c| c.set(c.get() + 1));
        Self(CAUGHT.with(|c| c.replace(None)))
    }

    #[inline(never)]
    fn take_message(&self) -> Option<String> {
        CAUGHT.with(|c| c.borrow_mut().take())
    }
}

impl Drop for PanicScope {
    #[inline(never)]
    fn drop(&mut self) {
        CONTAINED.with(|c| c.set(c.get() - 1));
        drop(CAUGHT.with(|c| c.replace(self.0.take())));
    }
}

struct SuspendedPanicScope {
    depth: u32,
    caught: Option<String>,
}

impl SuspendedPanicScope {
    #[inline(never)]
    fn enter() -> Self {
        let depth = CONTAINED.with(|c| c.replace(0));
        let caught = CAUGHT.with(|c| c.replace(None));
        Self { depth, caught }
    }
}

impl Drop for SuspendedPanicScope {
    #[inline(never)]
    fn drop(&mut self) {
        CONTAINED.with(|c| c.set(self.depth));
        drop(CAUGHT.with(|c| c.replace(self.caught.take())));
    }
}

/// Catch a real panic and retain its diagnostic, even if dropping its payload also panics.
/// The game panic hook records its attributed message while this boundary is active.
pub fn contain_unwind<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    let scope = PanicScope::enter();
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = scope
            .take_message()
            .unwrap_or_else(|| panic_payload_text(&*payload));
        // An opaque payload may run mod code when dropped. Never replace the original
        // diagnostic with that destructor's panic, or try to drop a second hostile payload.
        if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
            std::mem::forget(secondary);
        }
        message
    })
}

/// Called by the game panic hook to log and record a contained panic instead of a crash.
#[inline(never)]
pub fn record_contained_panic(diagnostic: &dyn Fn() -> String) -> bool {
    if CONTAINED.with(Cell::get) == 0 {
        return false;
    }
    let message = diagnostic();
    CAUGHT.with(|c| *c.borrow_mut() = Some(message));
    true
}

/// The printable message Rust gives string payloads; other payloads are opaque.
pub fn panic_payload_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Box<dyn Any>".into())
}

/// Whether the current panic will be reported and contained by the retained callback owner.
pub fn retained_callback_active() -> bool {
    RETAINED_CALLBACK.try_with(Cell::get).unwrap_or(false)
}

// Interrupt wrapper code without invoking the panic hook a second time.
fn unwind_through_wrapper() -> ! {
    struct DelegatedEnginePanic;
    std::panic::resume_unwind(Box::new(DelegatedEnginePanic))
}

fn engine_panic_pending() -> bool {
    DELEGATED_ENGINE_PANIC.with(|panic| panic.borrow().is_some())
}

fn drop_contained<T>(value: T) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(value))) {
        // A panic payload can itself have a panicking destructor.
        std::mem::forget(payload);
    }
}

#[derive(PartialEq, Eq)]
enum Invocation {
    Isolated,
    Listener,
    Dispose,
}

struct RetainedCallbackScope(bool);
impl Drop for RetainedCallbackScope {
    fn drop(&mut self) {
        RETAINED_CALLBACK.set(self.0);
    }
}

/// Drain pending callback panic notices queued since the last call.
pub fn take_callback_panic_notices() -> Vec<CallbackPanicNotice> {
    let mut queue = PANIC_NOTICES
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    queue.drain(..).collect()
}

fn record_callback_panic(
    owner: &'static str,
    kind: &'static str,
    operation: &'static str,
    message: String,
) {
    PANIC_NOTICES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push_back(CallbackPanicNotice {
            mod_id: owner,
            kind,
            operation,
            message,
        });
}

/// Owner identity and poison state for one retained mod callback.
// no wire: the callback's enablement and poison state belong to this process.
// exhaustive: engine-only callback bookkeeping; private fields prevent downstream construction.
#[derive(Clone, Debug)]
pub struct CallbackOwner {
    id: &'static str,
    index: Option<usize>,
    poisoned: Arc<AtomicBool>,
}

impl CallbackOwner {
    /// Bind a dispatch-attributed registration to its installed mod slot once, at freeze time.
    pub fn new(id: &'static str) -> Option<Self> {
        Some(Self::for_index(id, crate::__private::mod_index(id)?))
    }

    pub(crate) fn for_index(id: &'static str, index: usize) -> Self {
        Self {
            id,
            index: Some(index),
            poisoned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Run a delayed task using its installed owner's shared state. A refusal is logged and
    /// aborts just the transaction; only a panic returns a diagnostic for the server to relay.
    pub fn call_scheduled_task<H: crate::ServerHost>(
        id: &'static str,
        host: &mut H,
        invoke: impl FnOnce(&mut H),
    ) -> Result<(), String> {
        let state = crate::runtime::installed()
            .and_then(|installed| {
                installed
                    .by_id
                    .get(id)
                    .map(|&i| &installed.scheduled_tasks[i])
            })
            .map(|owner| (owner.index, owner.poisoned.as_ref()));
        // A real panic unwinds the transaction before `run_scheduled` catches it. A refusal stays
        // inside the transaction as an Err, including one the callback caught internally.
        match run_scheduled(id, state, "schedule_task", || {
            crate::run_transaction(host, |host, _| contain_refusal(id, || invoke(host)))
        }) {
            Err(ScheduledFailure::Panicked(diagnostic)) => Err(diagnostic),
            _ => Ok(()),
        }
    }

    /// Bind a callback owner by mod id, resolving its installed slot when present.
    pub fn for_mod(id: &'static str) -> Self {
        Self {
            id,
            index: crate::__private::mod_index(id),
            poisoned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The owning mod's identifier.
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Whether this callback is still enabled for this session.
    pub fn is_enabled(&self) -> bool {
        enabled_opt(self.index, &self.poisoned)
    }

    /// Invoke retained code with its owner's enablement, attribution and panic policy.
    pub fn call<T>(
        &self,
        kind: &'static str,
        operation: &'static str,
        invoke: impl FnOnce() -> T,
    ) -> Result<T, String> {
        call_opt(
            self.id,
            self.index,
            &self.poisoned,
            kind,
            operation,
            Invocation::Isolated,
            invoke,
        )
        .map_err(|why| format!("mod {}: {why}", self.id))
    }

    /// Render a returned API error wrapper's causes while the callback still owns the
    /// invocation. Keep the original error outside the formatting unwind so a panicking
    /// destructor cannot collide with a formatter panic. Only engine-owned text escapes.
    pub fn call_with_error<T, E: std::error::Error + 'static>(
        &self,
        kind: &'static str,
        operation: &'static str,
        invoke: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, String>, String> {
        let mut returned_error = None;
        let result = self.call(kind, operation, || {
            invoke().map_err(|error| {
                use std::fmt::Write;
                let error: &E = returned_error.insert(error);
                let mut cause = error.source().unwrap_or(error);
                let mut text = String::new();
                let mut seen = Vec::new();
                loop {
                    if seen.iter().any(|&prev| std::ptr::eq(prev, cause)) {
                        text.push_str(": [cyclic error source]");
                        break;
                    }
                    if !seen.is_empty() {
                        text.push_str(": ");
                    }
                    seen.push(cause);
                    if write!(text, "{cause}").is_err() {
                        text.push_str("[error formatting cause]");
                    }
                    let Some(next) = cause.source() else { break };
                    cause = next;
                }
                text
            })
        });
        if let Some(error) = returned_error {
            self.dispose(kind, operation, error);
        }
        result
    }

    /// A retained callback invoked by a generated registration listener shares that
    /// listener's phase scope so a phase refusal can roll back its staged registrations.
    pub(crate) fn call_in_listener<T>(
        &self,
        kind: &'static str,
        operation: &'static str,
        invoke: impl FnOnce() -> T,
    ) -> Result<T, String> {
        call_opt(
            self.id,
            self.index,
            &self.poisoned,
            kind,
            operation,
            Invocation::Listener,
            invoke,
        )
        .map_err(|why| format!("mod {}: {why}", self.id))
    }

    /// Dispose of an owned mod value under its panic policy, even if ordinary calls are disabled.
    pub fn dispose<T>(&self, kind: &'static str, operation: &'static str, value: T) {
        let _ = call_opt(
            self.id,
            self.index,
            &self.poisoned,
            kind,
            operation,
            Invocation::Dispose,
            || drop(value),
        );
    }

    /// Execute the engine body delegated by a retained wrapper, outside all mod scopes.
    pub fn call_engine<T>(invoke: impl FnOnce() -> T) -> T {
        if !retained_callback_active() {
            return invoke();
        }
        if engine_panic_pending() {
            unwind_through_wrapper();
        }
        let _contained = RetainedCallbackScope(RETAINED_CALLBACK.replace(false));
        let _suspended = SuspendedPanicScope::enter();
        let _guard = crate::dispatch_scope::ModGuard::unattributed();
        let outer = phase_refusal::suspend_scope();
        let result = catch_unwind(AssertUnwindSafe(invoke));
        let _ = phase_refusal::leave_scope(outer);
        result.unwrap_or_else(|payload| {
            DELEGATED_ENGINE_PANIC.set(Some(payload));
            unwind_through_wrapper();
        })
    }
}

pub(crate) fn enabled(index: usize, poisoned: &AtomicBool) -> bool {
    enabled_opt(Some(index), poisoned)
}

fn enabled_opt(index: Option<usize>, poisoned: &AtomicBool) -> bool {
    index.is_none_or(crate::mod_index_on) && !poisoned.load(Ordering::Acquire)
}

pub(crate) fn call<T>(
    owner: &'static str,
    index: usize,
    poisoned: &AtomicBool,
    kind: &'static str,
    operation: &'static str,
    invoke: impl FnOnce() -> T,
) -> Result<T, String> {
    call_opt(
        owner,
        Some(index),
        poisoned,
        kind,
        operation,
        Invocation::Isolated,
        invoke,
    )
}

enum ScheduledFailure {
    Disabled,
    Refused,
    Panicked(String),
}

fn run_scheduled<T>(
    owner: &'static str,
    state: Option<(Option<usize>, &AtomicBool)>,
    operation: &'static str,
    invoke: impl FnOnce() -> Result<T, PhaseRefusal>,
) -> Result<T, ScheduledFailure> {
    let _guard = crate::mark_mod(owner, operation);
    match contain_unwind(|| {
        if state.is_some_and(|(index, poisoned)| enabled_opt(index, poisoned)) {
            invoke().map(Some)
        } else {
            // Captures also belong to the owner if the callable is discarded without running.
            drop(invoke);
            Ok(None)
        }
    }) {
        Ok(Ok(Some(value))) => Ok(value),
        Ok(Ok(None)) => Err(ScheduledFailure::Disabled),
        Ok(Err(refusal)) => {
            refusal.report_each();
            Err(ScheduledFailure::Refused)
        }
        Err(diagnostic) => {
            if let Some((_, poisoned)) = state {
                poisoned.store(true, Ordering::Release);
            }
            Err(ScheduledFailure::Panicked(diagnostic))
        }
    }
}

fn call_opt<T>(
    owner: &'static str,
    index: Option<usize>,
    poisoned: &AtomicBool,
    kind: &'static str,
    operation: &'static str,
    invocation: Invocation,
    invoke: impl FnOnce() -> T,
) -> Result<T, String> {
    if invocation != Invocation::Dispose && !enabled_opt(index, poisoned) {
        let _guard = crate::mark_mod(owner, operation);
        let _ = contain_unwind(|| drop(invoke));
        return Err(format!("{kind} owner is disabled"));
    }
    let _guard = crate::mark_mod(owner, operation);
    let contained = RetainedCallbackScope(RETAINED_CALLBACK.replace(true));
    let scope = PanicScope::enter();
    // A refusal by the phase gate refuses this call and nothing more; a real panic disables
    // this callback for the session, logs loudly and notifies the client error toast queue.
    let result = catch_unwind(AssertUnwindSafe(|| {
        if invocation == Invocation::Listener {
            Ok(invoke())
        } else {
            contain_refusal(owner, invoke)
        }
    }));
    if engine_panic_pending() {
        // Ignore the wrapper's outcome, even if it caught the unwind and returned normally.
        drop_contained(result);
        drop(scope);
        if contained.0 {
            unwind_through_wrapper();
        }
        drop(contained);
        let payload = DELEGATED_ENGINE_PANIC.take().expect("pending engine panic");
        std::panic::resume_unwind(payload);
    }
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(refusal)) => {
            refusal.report_each();
            Err(format!("{kind} callback refused: {refusal}"))
        }
        Err(payload) => {
            if invocation == Invocation::Listener && payload.is::<PhaseRefusal>() {
                // The enclosing listener's scope already latched this refusal.
                drop(scope);
                drop(contained);
                std::panic::resume_unwind(payload);
            }
            let first = !poisoned.swap(true, Ordering::AcqRel);
            let msg = scope
                .take_message()
                .unwrap_or_else(|| panic_payload_text(payload.as_ref()));
            drop_contained(payload);
            // Disposal is mandatory. Each instance's destructor is run at most once,
            // but even an already disabled implementation can panic again while dropping.
            if !first && invocation != Invocation::Dispose {
                return Err(format!("{kind} callback panicked; it is disabled"));
            }
            log::error!(
                "mod {owner}: {kind} callback ({operation}) panicked and is disabled for this session: {msg}"
            );
            record_callback_panic(owner, kind, operation, msg.clone());
            Err(format!("{kind} callback panicked; it is disabled: {msg}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::phase::ModPhase;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn returned_native_errors_keep_causes_and_contain_format_and_drop_panics() {
        use crate::FeatureError;
        use std::{error::Error, fmt};

        #[derive(Debug)]
        struct Cycle;
        impl fmt::Display for Cycle {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("cyclic cause")
            }
        }
        impl Error for Cycle {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(self)
            }
        }

        #[derive(Debug)]
        struct Hostile<'a> {
            format_panics: bool,
            drops: &'a AtomicUsize,
        }
        impl fmt::Display for Hostile<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                assert!(!self.format_panics, "panic in error display");
                f.write_str("drop-only error")
            }
        }
        impl Error for Hostile<'_> {}
        impl Drop for Hostile<'_> {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::Relaxed);
                panic!("panic in error drop");
            }
        }

        let kind = "feature type";
        let operation = "place test";
        let other = CallbackOwner::for_mod("returned_error_test");
        let cycle = other
            .call_with_error(kind, operation, || Err::<(), _>(FeatureError::new(Cycle)))
            .unwrap()
            .unwrap_err();
        assert_eq!(cycle, "cyclic cause: [cyclic error source]");

        static DROPS: AtomicUsize = AtomicUsize::new(0);
        for format_panics in [false, true] {
            let owner = CallbackOwner::for_mod("returned_error_test");
            let result = owner.call_with_error(kind, operation, || {
                Err::<(), _>(FeatureError::new(Hostile {
                    format_panics,
                    drops: &DROPS,
                }))
            });
            if format_panics {
                assert!(result.unwrap_err().contains("panic in error display"));
            } else {
                assert_eq!(result.unwrap().unwrap_err(), "drop-only error");
            }
            assert!(!owner.is_enabled());
            assert_eq!(other.call(kind, operation, || 17), Ok(17));
        }
        assert_eq!(DROPS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn identical_phase_refusals_are_each_reported_without_disabling_the_callback() {
        const OWNER: &str = "test_repeated_retained_refusal";
        const OPERATION: &str = "test_retained_phase_gate";
        struct RefusalLog(AtomicUsize);
        impl log::Log for RefusalLog {
            fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                metadata.level() == log::Level::Warn
            }
            fn log(&self, record: &log::Record<'_>) {
                if self.enabled(record.metadata()) {
                    let message = record.args().to_string();
                    if message.contains(OWNER) && message.contains(OPERATION) {
                        self.0.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            fn flush(&self) {}
        }
        static LOG: RefusalLog = RefusalLog(AtomicUsize::new(0));
        log::set_logger(&LOG).unwrap();
        log::set_max_level(log::LevelFilter::Warn);

        let owner = CallbackOwner::for_mod(OWNER);
        for reports in [1, 2] {
            assert!(
                owner
                    .call::<()>("test", "retained_test", || {
                        phase_refusal::refuse(OPERATION, ModPhase::Setup);
                    })
                    .is_err()
            );
            assert_eq!(LOG.0.load(Ordering::Relaxed), reports);
            assert!(owner.is_enabled());
        }
        assert_eq!(owner.call("test", "retained_test", || 17), Ok(17));
    }

    #[test]
    #[ignore = "daily"]
    fn a_panicking_payload_destructor_preserves_the_original_callback_diagnostic() {
        use std::sync::{Arc, Mutex};

        // The process-wide hook must not replace the hook of concurrent tests.
        const CHILD: &str = "SUNRISE_CALLBACK_PANIC_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "callback_owner::tests::a_panicking_payload_destructor_preserves_the_original_callback_diagnostic",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("test result: ok. 1 passed;"),
                "the isolated panic-hook test did not pass exactly one test:\n{stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        const ORIGINAL: &str = "original opaque callback panic";
        const DESTRUCTOR: &str = "panic in the payload destructor";
        struct PanicOnDrop(Arc<AtomicBool>);
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
                std::panic::panic_any(DESTRUCTOR);
            }
        }

        let recorded = Arc::new(Mutex::new(Vec::new()));
        let seen = recorded.clone();
        std::panic::set_hook(Box::new(move |info| {
            let diagnostic = if info.payload().is::<PanicOnDrop>() {
                ORIGINAL
            } else if info.payload().downcast_ref::<&str>().copied() == Some(DESTRUCTOR) {
                DESTRUCTOR
            } else {
                return;
            };
            let contained = record_contained_panic(&|| diagnostic.to_owned());
            seen.lock().unwrap().push((diagnostic, contained));
        }));

        let dropped = Arc::new(AtomicBool::new(false));
        let poisoned = AtomicBool::new(false);
        let result: Result<(), _> = call("owner", 0, &poisoned, "test", "test", || {
            std::panic::panic_any(PanicOnDrop(dropped.clone()));
        });
        drop(std::panic::take_hook());
        assert!(dropped.load(Ordering::Relaxed));
        assert_eq!(
            recorded.lock().unwrap().as_slice(),
            [(ORIGINAL, true), (DESTRUCTOR, true)]
        );
        assert_eq!(
            result.unwrap_err(),
            "test callback panicked; it is disabled: original opaque callback panic"
        );
        assert!(poisoned.load(Ordering::Acquire));
    }
}
