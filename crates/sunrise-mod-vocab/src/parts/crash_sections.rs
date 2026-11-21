// exports: none (reached inside the crate only)
//! Crash report lines a mod adds (NeoForge's `CrashReportCallables`), computed when a report is
//! written rather than when they are registered.
use super::*;

/// A crash report line a mod computes when the report is written: NeoForge's
/// `ISystemReportExtender`, `ICrashReportHeader`, or a `registerCrashCallable` supplier with its
/// active check. The report asks each enabled callback once, on a separate thread, while the
/// game may have stopped mid-tick. Read the mod's own state without waiting on the game thread.
pub trait CrashSection: Send + Sync + 'static {
    /// The full text, or `None` to omit this entry. The report marks continuation lines so the
    /// text remains attributed to this entry.
    fn value(&self) -> Option<String>;
}

/// The registration side of [`CrashSection`]: what [`Registrar`] adds them with.
pub trait CrashSectionsRegistrar {
    /// `registerCrashCallable(label, supplier)`: a system detail, `<mod id>: <label>: <value>`,
    /// after Sunrise's own lines, in registration order. The registering mod's details are
    /// included even if the crash is attributed to that mod. A failed call records `~~ERROR~~`
    /// and its diagnostic for that entry. A panic also logs the mod, queues an error toast, and
    /// disables only this registration for the rest of the session.
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

/// Enabled crash report entries in registration order. Each has its registering mod's ID,
/// its label (`None` for a header), and its callable under the shared callback boundary.
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
