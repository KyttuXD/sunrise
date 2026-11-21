//! Client tooltip component factories (`RegisterClientTooltipComponentFactoriesEvent`,
//! NeoForge's `ClientTooltipComponentManager`): maps custom [`mod_api::TooltipComponentData`] types
//! to [`PreparedTooltipComponent`]s once at client startup.

use mod_api::{
    __private::game::CallbackOwner, ClientTooltipComponentFactoriesRegistrar,
    PreparedTooltipComponent, TooltipComponent, TooltipComponentFactory,
    content::crash::current_native_mod,
};
#[cfg(any(test, feature = "test-tooltip-factories"))]
use std::cell::RefCell;
use std::{any::TypeId, collections::HashMap, sync::OnceLock};

#[derive(Clone)]
struct RegisteredFactory {
    owner: Option<CallbackOwner>,
    factory: TooltipComponentFactory,
}

/// Client startup registrar collecting custom tooltip component factories before freeze.
#[derive(Default)]
pub struct TooltipComponentFactories {
    factories: HashMap<TypeId, RegisteredFactory>,
}

impl ClientTooltipComponentFactoriesRegistrar for TooltipComponentFactories {
    fn register_factory(&mut self, factory: TooltipComponentFactory) {
        let owner = current_native_mod().map(CallbackOwner::for_mod);
        self.factories
            .insert(factory.__type_id(), RegisteredFactory { owner, factory });
    }
}

/// Frozen lookup table of client tooltip component factories published at client startup.
pub struct FrozenTooltipComponentFactories {
    factories: HashMap<TypeId, RegisteredFactory>,
}

static FACTORIES: OnceLock<FrozenTooltipComponentFactories> = OnceLock::new();

#[cfg(any(test, feature = "test-tooltip-factories"))]
thread_local! {
    static TEST_FACTORIES: RefCell<Option<FrozenTooltipComponentFactories>> = const { RefCell::new(None) };
}

/// Publish the client startup tooltip component factory table once, before any tooltip renders.
pub fn freeze(factories: TooltipComponentFactories) {
    if FACTORIES
        .set(FrozenTooltipComponentFactories {
            factories: factories.factories,
        })
        .is_err()
    {
        log::warn!("tooltip component factories were registered already; these are left out");
    }
}

/// Install scoped tooltip component factories on this thread for the duration of `f`, restoring
/// the previous value on return or unwind.
#[cfg(any(test, feature = "test-tooltip-factories"))]
pub fn with_test_factories<R>(factories: TooltipComponentFactories, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<FrozenTooltipComponentFactories>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_FACTORIES.with(|slot| {
                *slot.borrow_mut() = self.0.take();
            });
        }
    }
    let previous = TEST_FACTORIES.with(|slot| {
        slot.replace(Some(FrozenTooltipComponentFactories {
            factories: factories.factories,
        }))
    });
    let _guard = Restore(previous);
    f()
}

/// A payload whose concrete type has no registered client factory.
#[derive(Debug, PartialEq, Eq)]
pub struct MissingTooltipComponentFactory {
    pub producer: Option<&'static str>,
    pub type_name: &'static str,
}

impl std::fmt::Display for MissingTooltipComponentFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(producer) = self.producer {
            write!(f, "mod {producer}: ")?;
        }
        write!(
            f,
            "unknown tooltip component {} (no client factory)",
            self.type_name
        )
    }
}

/// Prepare a custom payload. A registered factory's isolated failure omits just that
/// renderer; an unsupported type fails tooltip preparation, as in `ClientTooltipComponent.create`.
pub fn create(
    component: &TooltipComponent,
) -> Result<Option<PreparedTooltipComponent>, MissingTooltipComponentFactory> {
    let (Some(type_id), Some(type_name), Some(data)) = (
        component.type_id(),
        component.type_name(),
        component.__as_any(),
    ) else {
        return Ok(None); // Built-in bundle data is prepared by the game, not the factory table.
    };
    let missing = || MissingTooltipComponentFactory {
        producer: component.__producer(),
        type_name,
    };
    #[cfg(any(test, feature = "test-tooltip-factories"))]
    if let Some(found) = TEST_FACTORIES.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|frozen| frozen.factories.get(&type_id).cloned())
    }) {
        let entry = found.ok_or_else(missing)?;
        return Ok(entry.factory.__create(data, entry.owner));
    }
    let entry = FACTORIES
        .get()
        .and_then(|frozen| frozen.factories.get(&type_id))
        .ok_or_else(missing)?;
    Ok(entry.factory.__create(data, entry.owner.clone()))
}

/// Whether any custom tooltip component factory is registered in the active table.
pub fn has_factories() -> bool {
    #[cfg(any(test, feature = "test-tooltip-factories"))]
    if let Some(has) = TEST_FACTORIES.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|frozen| !frozen.factories.is_empty())
    }) {
        return has;
    }
    FACTORIES
        .get()
        .is_some_and(|frozen| !frozen.factories.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::{
        ClientTooltipComponent, Identifier, Text, TextFormError, TooltipComponentData, TooltipFont,
    };

    struct MetricData(i32);
    impl TooltipComponentData for MetricData {}

    struct UnregisteredData;
    impl TooltipComponentData for UnregisteredData {}

    struct MetricClient {
        width: i32,
        height: i32,
    }
    impl ClientTooltipComponent for MetricClient {
        fn height(&self, _font: &dyn TooltipFont) -> i32 {
            self.height
        }
        fn width(&self, _font: &dyn TooltipFont) -> i32 {
            self.width
        }
    }

    struct StubFont(Identifier);
    impl TooltipFont for StubFont {
        fn id(&self) -> &Identifier {
            &self.0
        }
        fn line_height(&self) -> i32 {
            9
        }
        fn width(&self, _text: &Text) -> Result<i32, TextFormError> {
            Ok(0)
        }
        fn split(&self, _text: &Text, _max_width: i32) -> Result<Vec<Text>, TextFormError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn factories_create_replace_reject_unregistered_and_restore_on_unwind() {
        let font = StubFont(Identifier::parse("minecraft:default").unwrap());
        let data = TooltipComponent::custom(MetricData(12));
        let unknown = TooltipComponent::custom(UnregisteredData);

        with_test_factories(TooltipComponentFactories::default(), || {
            assert!(!has_factories());
            assert!(create(&data).is_err());

            let mut factories = TooltipComponentFactories::default();
            let registrar: &mut dyn ClientTooltipComponentFactoriesRegistrar = &mut factories;
            registrar.register::<MetricData, MetricClient>(|d| MetricClient {
                width: d.0,
                height: 10,
            });
            // Last registration wins, matching NeoForge HashMap::put.
            registrar.register::<MetricData, MetricClient>(|d| MetricClient {
                width: d.0 * 3,
                height: 20,
            });

            with_test_factories(factories, || {
                assert!(has_factories());
                let prepared = create(&data)
                    .unwrap()
                    .expect("registered factory should prepare component");
                assert_eq!(prepared.width(&font), 36);
                assert_eq!(prepared.height(&font), 20);
                assert!(create(&unknown).is_err());
            });

            // Inner scope restored outer empty table.
            assert!(!has_factories());
            assert!(create(&data).is_err());

            // Unwind safety: a panic inside with_test_factories still restores the prior table.
            let mut inner = TooltipComponentFactories::default();
            let registrar: &mut dyn ClientTooltipComponentFactoriesRegistrar = &mut inner;
            registrar.register::<MetricData, MetricClient>(|d| MetricClient {
                width: d.0,
                height: 1,
            });
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_test_factories(inner, || {
                    assert!(has_factories());
                    panic!("expected unwind");
                });
            }));
            assert!(panicked.is_err());
            assert!(!has_factories());
        });
    }

    #[test]
    fn panicking_factory_or_component_methods_disable_only_that_owner() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        struct PanicCreateData;
        impl TooltipComponentData for PanicCreateData {}
        struct PanicWidthData;
        impl TooltipComponentData for PanicWidthData {}
        struct PanicHeightData;
        impl TooltipComponentData for PanicHeightData {}
        struct PanicHandData;
        impl TooltipComponentData for PanicHandData {}
        struct HealthyData(i32);
        impl TooltipComponentData for HealthyData {}

        struct PanicWidthClient(Arc<AtomicUsize>);
        impl ClientTooltipComponent for PanicWidthClient {
            fn height(&self, _font: &dyn TooltipFont) -> i32 {
                10
            }
            fn width(&self, _font: &dyn TooltipFont) -> i32 {
                self.0.fetch_add(1, Ordering::Relaxed);
                panic!("width boom");
            }
        }

        struct PanicHeightClient(Arc<AtomicUsize>);
        impl ClientTooltipComponent for PanicHeightClient {
            fn height(&self, _font: &dyn TooltipFont) -> i32 {
                self.0.fetch_add(1, Ordering::Relaxed);
                panic!("height boom");
            }
            fn width(&self, _font: &dyn TooltipFont) -> i32 {
                10
            }
        }

        struct PanicHandClient(Arc<AtomicUsize>);
        impl ClientTooltipComponent for PanicHandClient {
            fn height(&self, _font: &dyn TooltipFont) -> i32 {
                10
            }
            fn width(&self, _font: &dyn TooltipFont) -> i32 {
                10
            }
            fn show_tooltip_with_item_in_hand(&self) -> bool {
                self.0.fetch_add(1, Ordering::Relaxed);
                panic!("hand boom");
            }
        }

        let create_calls = Arc::new(AtomicUsize::new(0));
        let width_calls = Arc::new(AtomicUsize::new(0));
        let height_calls = Arc::new(AtomicUsize::new(0));
        let hand_calls = Arc::new(AtomicUsize::new(0));

        let mut factories = TooltipComponentFactories::default();
        let insert_owned = |factories: &mut TooltipComponentFactories,
                            mod_id: &'static str,
                            factory: TooltipComponentFactory| {
            factories.factories.insert(
                factory.__type_id(),
                RegisteredFactory {
                    owner: Some(CallbackOwner::for_mod(mod_id)),
                    factory,
                },
            );
        };

        let c_calls = create_calls.clone();
        insert_owned(
            &mut factories,
            "panic_create_mod",
            TooltipComponentFactory::new::<PanicCreateData, MetricClient>(move |_| {
                c_calls.fetch_add(1, Ordering::Relaxed);
                panic!("create boom");
            }),
        );
        let w_calls = width_calls.clone();
        insert_owned(
            &mut factories,
            "panic_width_mod",
            TooltipComponentFactory::new::<PanicWidthData, PanicWidthClient>(move |_| {
                PanicWidthClient(w_calls.clone())
            }),
        );
        let h_calls = height_calls.clone();
        insert_owned(
            &mut factories,
            "panic_height_mod",
            TooltipComponentFactory::new::<PanicHeightData, PanicHeightClient>(move |_| {
                PanicHeightClient(h_calls.clone())
            }),
        );
        let s_calls = hand_calls.clone();
        insert_owned(
            &mut factories,
            "panic_hand_mod",
            TooltipComponentFactory::new::<PanicHandData, PanicHandClient>(move |_| {
                PanicHandClient(s_calls.clone())
            }),
        );
        insert_owned(
            &mut factories,
            "healthy_mod",
            TooltipComponentFactory::new::<HealthyData, MetricClient>(|d| MetricClient {
                width: d.0,
                height: 14,
            }),
        );

        let font = StubFont(Identifier::parse("minecraft:default").unwrap());
        with_test_factories(factories, || {
            let panic_create = TooltipComponent::custom(PanicCreateData);
            assert!(create(&panic_create).unwrap().is_none());
            assert_eq!(create_calls.load(Ordering::Relaxed), 1);
            // Disabled after first panic: subsequent create skips invoking the factory.
            assert!(create(&panic_create).unwrap().is_none());
            assert_eq!(create_calls.load(Ordering::Relaxed), 1);

            let panic_width = TooltipComponent::custom(PanicWidthData);
            let prepared = create(&panic_width)
                .unwrap()
                .expect("factory succeeds before width panics");
            assert_eq!(prepared.width(&font), 0);
            assert_eq!(width_calls.load(Ordering::Relaxed), 1);
            assert_eq!(prepared.width(&font), 0);
            assert_eq!(width_calls.load(Ordering::Relaxed), 1);
            assert!(create(&panic_width).unwrap().is_none());

            let panic_height = TooltipComponent::custom(PanicHeightData);
            let prepared = create(&panic_height)
                .unwrap()
                .expect("factory succeeds before height panics");
            assert_eq!(prepared.height(&font), 0);
            assert_eq!(height_calls.load(Ordering::Relaxed), 1);
            assert_eq!(prepared.height(&font), 0);
            assert_eq!(height_calls.load(Ordering::Relaxed), 1);
            assert!(create(&panic_height).unwrap().is_none());

            let panic_hand = TooltipComponent::custom(PanicHandData);
            let prepared = create(&panic_hand)
                .unwrap()
                .expect("factory succeeds before hand check panics");
            assert!(!prepared.show_tooltip_with_item_in_hand());
            assert_eq!(hand_calls.load(Ordering::Relaxed), 1);
            assert!(!prepared.show_tooltip_with_item_in_hand());
            assert_eq!(hand_calls.load(Ordering::Relaxed), 1);
            assert!(create(&panic_hand).unwrap().is_none());

            // Sibling mod factory remains completely unaffected.
            let healthy = TooltipComponent::custom(HealthyData(48));
            let prepared = create(&healthy)
                .unwrap()
                .expect("healthy sibling remains enabled");
            assert_eq!(prepared.width(&font), 48);
            assert_eq!(prepared.height(&font), 14);
        });
    }
}
