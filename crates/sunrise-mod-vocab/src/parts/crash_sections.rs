// exports: none (reached inside the crate only)
//! Crash report lines a mod adds (NeoForge's `CrashReportCallables`), computed when a report is
//! written rather than when they are registered.
use super::*;

/// A crash report line a mod computes when the report is written: NeoForge's
/// `ISystemReportExtender`, `ICrashReportHeader`, or a `registerCrashCallable` supplier with its
/// active check. The report asks each callback in registration order within its section,
/// on a worker, while the game may have stopped mid-tick. If a callback doesn't answer, the
/// report records that failure and moves on without waiting for the worker to finish.
pub trait CrashSection: Send + Sync + 'static {
    /// The full text to include in the report, or `None` to omit this entry. The report
    /// marks continuation lines so the text remains attributed to this entry.
    fn value(&self) -> Option<String>;
}

/// The registration side of [`CrashSection`]: what [`Registrar`] adds them with.
pub trait CrashSectionsRegistrar {
    /// `registerCrashCallable(label, supplier)`: a system detail, `<mod id>: <label>: <value>`,
    /// collected and written after Sunrise's own lines in registration order, even if the crash
    /// is attributed to the registering mod. Multiline labels and values have marked continuation
    /// lines. A failed value starts with `~~ERROR~~` without preventing other entries from being
    /// collected. A panic logs the mod, queues an error toast, and disables only this registration
    /// for the rest of the session.
    fn crash_section(&mut self, label: &str, section: Arc<dyn CrashSection>);
    /// `registerHeader`: an entry under the report's title, `<mod id>: <value>`, asked as a
    /// [`crash_section`](Self::crash_section) is.
    fn crash_header(&mut self, header: Arc<dyn CrashSection>);
}

// no wire: process-local callback and panic state.
#[derive(Clone)]
pub(crate) struct CrashSectionPart {
    pub(crate) mod_id: &'static str,
    pub(crate) mod_index: usize,
    /// The detail's label; `None` for a header line.
    pub(crate) label: Option<Box<str>>,
    pub(crate) section: Arc<dyn crate::CrashSection>,
    pub(crate) panicked: Arc<std::sync::atomic::AtomicBool>,
}

/// Enabled crash report entries in registration order: the mod's `[mod] id`, label
/// (`None` for a header), and the callable under the shared retained callback boundary.
pub fn mod_crash_lines(
    headers: bool,
) -> impl Iterator<
    Item = (
        &'static str,
        Option<&'static str>,
        impl FnOnce() -> Result<Option<String>, String> + Send + 'static,
    ),
> {
    let operation = if headers {
        "crash_header"
    } else {
        "crash_section"
    };
    installed()
        .into_iter()
        .flat_map(|installed| &installed.keys.crash_sections)
        .filter(move |line| {
            line.label.is_none() == headers
                && crate::callback_owner::enabled(line.mod_index, &line.panicked)
        })
        .map(move |line| {
            (line.mod_id, line.label.as_deref(), move || {
                crate::callback_owner::call(
                    line.mod_id,
                    line.mod_index,
                    &line.panicked,
                    "crash report",
                    operation,
                    || line.section.value(),
                )
            })
        })
}
