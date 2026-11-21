use super::screen_open_tests::{fresh_screen_case, with_screen_game, with_screen_window};
use crate::game::{Game, Screen, event_loop::EventLoopCtl, input_trace::InputOrigin};
use mod_api::{
    ClientHost, DrawContext, Flow, Hooks, ModEntry, ScreenCharTypedPostEvent, ScreenInitPostEvent,
    ScreenKeyPressedPostEvent, ScreenKeyReleasedPostEvent, ScreenMouseButtonPressedPostEvent,
    ScreenMouseButtonPressedPreEvent, ScreenMouseButtonReleasedPostEvent,
    ScreenMouseDraggedPostEvent, ScreenMouseScrolledPostEvent, ScreenPreeditPostEvent,
    ScreenWidget, ScreenWidgetId, Text,
    client::widgets::{Button, TextField},
    prelude::ScreensClientHost,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use sunrise_client::hud::HudBatch;
use winit::{
    event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    keyboard::KeyCode,
};

type Log<T> = Arc<Mutex<Vec<T>>>;
type Mount = dyn Fn(&dyn ClientHost, &mut ScreenInitPostEvent<'_, '_>) + Send + Sync;

const BUTTON: [f32; 4] = [10.0, 8.0, 80.0, 20.0];
const FIELD: [f32; 4] = [10.0, 32.0, 120.0, 20.0];
const CUSTOM: [f32; 4] = [10.0, 56.0, 80.0, 20.0];
const BUTTON_POINT: [f32; 2] = [16.0, 12.0];
const FIELD_POINT: [f32; 2] = [16.0, 36.0];
const CUSTOM_POINT: [f32; 2] = [16.0, 60.0];
const OUTSIDE: [f32; 2] = [140.0, 85.0];

#[derive(Debug, PartialEq)]
enum Input {
    PrePress(&'static str, i32, bool),
    Press(&'static str, i32, bool, bool),
    Release(&'static str, bool),
    Drag(&'static str),
    Scroll(&'static str),
    Char(&'static str, char),
    Preedit(&'static str, String),
    KeyDown(&'static str, String),
    KeyUp(&'static str, String),
}

#[derive(Default)]
struct InputProbe {
    events: Log<Input>,
    press_result: Mutex<Option<bool>>,
    cancel_press: AtomicBool,
    cancel_key: AtomicBool,
}

struct WidgetMod {
    mount: Box<Mount>,
    input: Arc<InputProbe>,
}

#[mod_api::__subscriptions]
impl Hooks for WidgetMod {
    fn screen_init_post(&self, host: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_, '_>) {
        (self.mount)(host, event);
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        self.input.events.lock().unwrap().push(Input::PrePress(
            event.screen.id,
            event.button,
            event.double_click,
        ));
        if self.input.cancel_press.load(Ordering::Relaxed) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn screen_mouse_button_pressed_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPostEvent<'_>,
    ) {
        self.input.events.lock().unwrap().push(Input::Press(
            event.screen.id,
            event.button,
            event.double_click,
            event.handled,
        ));
        if let Some(result) = *self.input.press_result.lock().unwrap() {
            event.handled = result;
        }
    }

    fn screen_mouse_button_released_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenMouseButtonReleasedPostEvent<'_>,
    ) {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::Release(event.screen.id, event.handled));
    }

    fn screen_mouse_dragged_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenMouseDraggedPostEvent<'_>,
    ) {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::Drag(event.screen.id));
    }

    fn screen_mouse_scrolled_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenMouseScrolledPostEvent<'_>,
    ) {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::Scroll(event.screen.id));
    }

    fn screen_char_typed_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenCharTypedPostEvent<'_>,
    ) {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::Char(event.screen.id, event.c));
    }

    fn screen_preedit_post(&self, _host: &dyn ClientHost, event: &mut ScreenPreeditPostEvent<'_>) {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::Preedit(event.screen.id, event.full_text.to_owned()));
    }

    fn screen_key_pressed_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenKeyPressedPostEvent<'_>,
    ) -> Flow {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::KeyDown(event.screen.id, event.key.to_owned()));
        event.cancelled = self.input.cancel_key.load(Ordering::Relaxed);
        Flow::Continue
    }

    fn screen_key_released_post(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenKeyReleasedPostEvent<'_>,
    ) -> Flow {
        self.input
            .events
            .lock()
            .unwrap()
            .push(Input::KeyUp(event.screen.id, event.key.to_owned()));
        event.cancelled = self.input.cancel_key.load(Ordering::Relaxed);
        Flow::Continue
    }
}

fn widget_mod(
    input: &Arc<InputProbe>,
    mount: impl Fn(&dyn ClientHost, &mut ScreenInitPostEvent<'_, '_>) + Send + Sync + 'static,
) -> ModEntry {
    ModEntry::new(
        "widget_probe",
        Box::new(WidgetMod {
            mount: Box::new(mount),
            input: input.clone(),
        }),
    )
}

fn take<T>(log: &Log<T>) -> Vec<T> {
    std::mem::take(&mut *log.lock().unwrap())
}

#[derive(Debug, PartialEq)]
enum WidgetCall {
    Focus(bool),
    Render { hovered: bool, focused: bool },
    Click(bool),
    Drag,
    Release,
    Scroll,
    KeyDown(String),
    KeyUp(String),
    Char(char),
    Drop,
}

struct Custom {
    log: Log<WidgetCall>,
    takes_input: Arc<AtomicBool>,
}

impl Drop for Custom {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(WidgetCall::Drop);
    }
}

impl Custom {
    fn record(&self, call: WidgetCall) -> bool {
        self.log.lock().unwrap().push(call);
        self.takes_input.load(Ordering::Relaxed)
    }
}

impl ScreenWidget for Custom {
    fn bounds(&self) -> [f32; 4] {
        CUSTOM
    }
    fn render(
        &mut self,
        _: &dyn ClientHost,
        _: &mut dyn DrawContext,
        _: [f32; 2],
        hovered: bool,
        focused: bool,
        _: f32,
    ) {
        self.record(WidgetCall::Render { hovered, focused });
    }
    fn mouse_clicked(
        &mut self,
        _: &mut dyn ClientHost,
        _: [f32; 2],
        _: i32,
        _: u32,
        double_click: bool,
    ) -> bool {
        self.record(WidgetCall::Click(double_click))
    }
    fn mouse_dragged(
        &mut self,
        _: &mut dyn ClientHost,
        _: [f32; 2],
        _: i32,
        _: [f32; 2],
        _: u32,
    ) -> bool {
        self.record(WidgetCall::Drag)
    }
    fn mouse_released(&mut self, _: &mut dyn ClientHost, _: [f32; 2], _: i32, _: u32) -> bool {
        self.record(WidgetCall::Release)
    }
    fn mouse_scrolled(&mut self, _: &mut dyn ClientHost, _: [f32; 2], _: [f64; 2], _: u32) -> bool {
        self.record(WidgetCall::Scroll)
    }
    fn key_pressed(&mut self, _: &mut dyn ClientHost, key: &str, _: bool, _: u32) -> bool {
        self.record(WidgetCall::KeyDown(key.to_owned()))
    }
    fn key_released(&mut self, _: &mut dyn ClientHost, key: &str, _: u32) -> bool {
        self.record(WidgetCall::KeyUp(key.to_owned()))
    }
    fn char_typed(&mut self, _: &mut dyn ClientHost, c: char, _: u32) -> bool {
        self.record(WidgetCall::Char(c))
    }
    fn set_focused(&mut self, focused: bool) {
        self.record(WidgetCall::Focus(focused));
    }
}

struct TestEventLoop;
impl EventLoopCtl for TestEventLoop {
    fn exit(&self) {}
    fn set_control_flow(&self, _: winit::event_loop::ControlFlow) {}
}

fn window_event(game: &mut Game<'_>, event: WindowEvent) {
    let id = game.window.as_ref().unwrap().id();
    game.handle_window_event(&TestEventLoop, id, event, InputOrigin::User);
}

fn move_pointer(game: &mut Game<'_>, [x, y]: [f32; 2]) {
    let position = game
        .renderer_pointer_projection()
        .unwrap()
        .window_pointer((x, y));
    window_event(
        game,
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
        },
    );
}

fn mouse(game: &mut Game<'_>, state: ElementState, button: MouseButton) {
    window_event(
        game,
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button,
        },
    );
}

fn click(game: &mut Game<'_>, point: [f32; 2]) {
    move_pointer(game, point);
    mouse(game, ElementState::Pressed, MouseButton::Left);
    mouse(game, ElementState::Released, MouseButton::Left);
}

// Drive the same Init.Post dispatch the window observer uses, without creating a renderer
// for tests concerned only with the mounted children's editing or callback behavior.
fn mount_headless(game: &mut Game<'_>, screen: Screen) {
    game.set_screen(screen);
    let view = mod_api::ScreenView::from(mod_api::__private::built::ScreenView {
        id: screen.id(),
        menu: None,
        title: None,
        rect: [0.0; 4],
        slots: 0,
        mouse: [0.0; 2],
        partial_tick: 0.0,
    });
    game.init_screen_widgets(&view);
}

fn field<'a>(game: &'a Game<'_>) -> &'a sunrise_client::ui::EditBox {
    let crate::game::screen_widgets::MountedWidgetKind::TextField { edit_box, .. } =
        &game.screen_state.widgets.entries[0].as_ref().unwrap().kind
    else {
        panic!("first mounted widget must be a text field")
    };
    edit_box
}

#[test]
fn double_click_requires_the_same_screen_instance_button_and_less_than_250_ms() {
    use crate::game::screen_build::ScreenState;
    let mut screen = ScreenState::default();
    let first = Instant::now();
    assert!(!screen.is_double_click(first, "mod", 0));
    screen.record_click(first, "mod", 0);
    let just_before = first + Duration::from_millis(249);
    assert!(screen.is_double_click(just_before, "mod", 0));
    assert!(!screen.is_double_click(first + Duration::from_millis(250), "mod", 0));
    assert!(!screen.is_double_click(just_before, "mod", 1));
    assert!(!screen.is_double_click(just_before, "inventory", 0));
    // The public id "mod" is shared by distinct screens. Even if the last click survives,
    // changing the screen instance must not turn the first press on the new screen into a double.
    screen.widgets.clear();
    assert!(!screen.is_double_click(just_before, "mod", 0));
}

#[test]
#[ignore = "daily"]
fn init_mounts_distinct_widgets_and_resize_and_close_dispose_them() {
    if fresh_screen_case(
        module_path!(),
        "init_mounts_distinct_widgets_and_resize_and_close_dispose_them",
    ) {
        return;
    }
    let input = Arc::new(InputProbe::default());
    let log = Log::default();
    let ids = Log::<[ScreenWidgetId; 3]>::default();
    let entry = widget_mod(&input, {
        let (log, ids) = (log.clone(), ids.clone());
        move |_, event| {
            if event.screen.id != "mod" {
                return;
            }
            let button = event
                .widgets
                .add_button(Button::inactive(BUTTON, Text::literal("Save")));
            let field = event.widgets.add_text_field(TextField::new(FIELD, 20));
            let custom = event.widgets.add_widget(Box::new(Custom {
                log: log.clone(),
                takes_input: Arc::new(AtomicBool::new(true)),
            }));
            ids.lock().unwrap().push([button, field, custom]);
            if ids.lock().unwrap().len() == 1 {
                assert!(event.widgets.set_initial_focus(custom));
            }
            assert!(event.widgets.remove(button));
            assert!(!event.widgets.remove(button));
            assert!(!event.widgets.set_initial_focus(button));
        }
    });
    with_screen_window(vec![entry].into(), |game| {
        assert!(game.mod_screen_open("widget_probe"));
        let [button, field, custom] = ids.lock().unwrap()[0];
        assert_ne!(button, field);
        assert_ne!(field, custom);
        assert_ne!(button, custom);
        assert_eq!(take(&log), [WidgetCall::Focus(true)]);
        assert!(game.mounted_widget_mouse_clicked(CUSTOM_POINT, 0, 0, false));
        assert!(game.screen_state.widgets.dragging.is_some());
        assert_eq!(take(&log), [WidgetCall::Click(false)]);
        assert_eq!(
            game.screen_state
                .widgets
                .entries
                .iter()
                .flatten()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            [field, custom]
        );

        game.renderer.as_mut().unwrap().resize(431, 301);
        game.observe_screen();
        assert_eq!(take(&log), [WidgetCall::Drop]);
        assert_eq!(ids.lock().unwrap().len(), 2);
        assert_ne!(ids.lock().unwrap()[1][2], custom);
        assert!(game.screen_state.widgets.focused.is_none());
        assert!(game.screen_state.widgets.dragging.is_none());
        assert!(game.mounted_widget_mouse_clicked(CUSTOM_POINT, 0, 0, false));
        assert_eq!(
            take(&log),
            [WidgetCall::Click(false), WidgetCall::Focus(true)]
        );

        assert!(game.close_screen());
        assert_eq!(game.screen, Screen::Playing);
        assert_eq!(take(&log), [WidgetCall::Drop]);
        assert!(game.screen_state.widgets.entries.is_empty());
        assert!(game.screen_state.widgets.focused.is_none());
        assert!(game.screen_state.widgets.dragging.is_none());
    });
}

#[test]
#[ignore = "daily"]
fn mounted_button_changes_state_on_click_enter_and_space() {
    if fresh_screen_case(
        module_path!(),
        "mounted_button_changes_state_on_click_enter_and_space",
    ) {
        return;
    }
    let labels = Log::default();
    let entry = widget_mod(&Arc::default(), {
        let labels = labels.clone();
        move |_, event| {
            if event.screen.id != "paused" {
                return;
            }
            let labels = labels.clone();
            event.widgets.add_button(Button::new(
                BUTTON,
                Text::literal("click"),
                move |state, _| {
                    let mut labels = labels.lock().unwrap();
                    labels.push(state.label.clone());
                    match labels.len() {
                        1 => state.label = Text::literal("enter"),
                        2 => state.label = Text::literal("space"),
                        _ => state.active = false,
                    }
                },
            ));
        }
    });
    with_screen_game(vec![entry].into(), |game| {
        mount_headless(game, Screen::Paused);
        assert!(game.mounted_widget_mouse_clicked(BUTTON_POINT, 0, 0, false));
        assert_eq!(&*labels.lock().unwrap(), &[Text::literal("click")]);
        assert!(game.mounted_widget_key_pressed(KeyCode::Enter, false, 0));
        assert!(game.mounted_widget_key_pressed(KeyCode::Space, false, 0));
        assert!(!game.mounted_widget_mouse_clicked(BUTTON_POINT, 0, 0, false));
        assert_eq!(
            take(&labels),
            [
                Text::literal("click"),
                Text::literal("enter"),
                Text::literal("space")
            ]
        );
    });
}

#[test]
#[ignore = "daily"]
fn mounted_text_field_edits_and_submits_without_drawing() {
    if fresh_screen_case(
        module_path!(),
        "mounted_text_field_edits_and_submits_without_drawing",
    ) {
        return;
    }
    let calls = Log::default();
    let entry = widget_mod(&Arc::default(), {
        let calls = calls.clone();
        move |_, event| {
            if event.screen.id != "mod" {
                return;
            }
            let (change, submit) = (calls.clone(), calls.clone());
            let id = event.widgets.add_text_field(
                TextField::new(FIELD, 30)
                    .value("abc")
                    .on_change(move |value, _| {
                        change.lock().unwrap().push(("change", value.to_owned()))
                    })
                    .on_submit(move |value, _| {
                        submit.lock().unwrap().push(("submit", value.to_owned()))
                    }),
            );
            assert!(event.widgets.set_initial_focus(id));
        }
    });
    with_screen_game(vec![entry].into(), |game| {
        mount_headless(game, Screen::Mod);
        assert!(game.text_field_focused());
        let key =
            |game: &mut Game<'_>, key| assert!(game.mounted_widget_key_pressed(key, false, 0));
        let ty = |game: &mut Game<'_>, c| assert!(game.mounted_widget_char_typed(c, 0));
        ty(game, 'd');
        key(game, KeyCode::ArrowLeft);
        key(game, KeyCode::Backspace);
        key(game, KeyCode::Delete);
        key(game, KeyCode::Home);
        ty(game, 'x');
        key(game, KeyCode::ArrowRight);
        ty(game, 'y');
        key(game, KeyCode::End);
        ty(game, 'z');
        let control_or_command = if cfg!(target_os = "macos") { 8 } else { 2 };
        assert!(game.mounted_widget_key_pressed(KeyCode::KeyA, false, control_or_command));
        assert_eq!(field(game).selected_text(), "xaybz");
        ty(game, 'Q');
        key(game, KeyCode::Enter);
        assert_eq!(field(game).value, "Q");
        assert_eq!(
            take(&calls)
                .iter()
                .map(|(op, value)| (*op, value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("change", "abcd"),
                ("change", "abd"),
                ("change", "ab"),
                ("change", "xab"),
                ("change", "xayb"),
                ("change", "xaybz"),
                ("change", "Q"),
                ("submit", "Q"),
            ]
        );
    });
}

#[derive(Debug, PartialEq)]
enum Transition {
    InitA,
    InitB,
    Returned(bool),
    DropA,
    PressB,
}

struct DropButton(Log<Transition>);
impl Drop for DropButton {
    fn drop(&mut self) {
        self.0.lock().unwrap().push(Transition::DropA);
    }
}

#[test]
#[ignore = "daily"]
fn button_transitions_are_synchronous_and_post_stays_on_the_receiver() {
    if fresh_screen_case(
        module_path!(),
        "button_transitions_are_synchronous_and_post_stays_on_the_receiver",
    ) {
        return;
    }
    let input = Arc::new(InputProbe::default());
    let transitions = Log::default();
    let close = Arc::new(AtomicBool::new(false));
    let entry = widget_mod(&input, {
        let (transitions, close) = (transitions.clone(), close.clone());
        move |host, event| {
            // A second run uses pause as the receiver: both native mod screens have id
            // "mod", so the pause->mod change distinguishes a stale receiver snapshot.
            let owner = ScreensClientHost::mod_screen(host);
            if owner == Some("mod_a") || event.screen.id == "paused" {
                transitions.lock().unwrap().push(Transition::InitA);
                let drop_button = DropButton(transitions.clone());
                let (transitions, close) = (transitions.clone(), close.clone());
                event.widgets.add_button(Button::new(
                    BUTTON,
                    Text::literal("A"),
                    move |_, host| {
                        let _keep_until_disposal = &drop_button;
                        let changed = if close.load(Ordering::Relaxed) {
                            ScreensClientHost::close_screen(host)
                        } else {
                            ScreensClientHost::mod_screen_open(host, "mod_b")
                        };
                        transitions
                            .lock()
                            .unwrap()
                            .push(Transition::Returned(changed));
                    },
                ));
            } else if owner == Some("mod_b") {
                transitions.lock().unwrap().push(Transition::InitB);
                let transitions = transitions.clone();
                event
                    .widgets
                    .add_button(Button::new(BUTTON, Text::literal("B"), move |_, _| {
                        transitions.lock().unwrap().push(Transition::PressB);
                    }));
            }
        }
    });
    with_screen_window(vec![entry].into(), |game| {
        use Transition::*;
        for (receiver, close_screen) in [("mod", false), ("paused", false), ("mod", true)] {
            close.store(close_screen, Ordering::Relaxed);
            if receiver == "paused" {
                game.set_screen(Screen::Paused);
                game.observe_screen();
            } else {
                assert!(game.mod_screen_open("mod_a"));
            }
            assert_eq!(take(&transitions), [InitA]);
            move_pointer(game, BUTTON_POINT);
            mouse(game, ElementState::Pressed, MouseButton::Left);
            assert_eq!(
                take(&input.events),
                [
                    Input::PrePress(receiver, 0, false),
                    Input::Press(receiver, 0, false, true)
                ]
            );
            assert!(game.screen_state.last_click.is_none());
            if close_screen {
                assert_eq!(take(&transitions), [Returned(true), DropA]);
                assert_eq!(game.screen, Screen::Playing);
                assert!(game.screen_state.widgets.entries.is_empty());
            } else {
                assert_eq!(take(&transitions), [InitB, Returned(true), DropA]);
                assert_eq!(game.mod_screen(), Some("mod_b"));
                assert!(game.mounted_widget_mouse_clicked(BUTTON_POINT, 0, 0, false));
                assert_eq!(take(&transitions), [PressB]);
            }
            mouse(game, ElementState::Released, MouseButton::Left);
            take(&input.events);
        }
    });
}

#[test]
#[ignore = "daily"]
fn ime_and_double_click_use_the_focused_edit_box() {
    if fresh_screen_case(
        module_path!(),
        "ime_and_double_click_use_the_focused_edit_box",
    ) {
        return;
    }
    let input = Arc::new(InputProbe::default());
    let edits = Log::default();
    let entry = widget_mod(&input, {
        let edits = edits.clone();
        move |_, event| {
            if event.screen.id != "mod" {
                return;
            }
            let read_only = [10.0, 8.0, 80.0, 20.0];
            let edits = edits.clone();
            let id = event.widgets.add_text_field(
                TextField::new(FIELD, 40)
                    .value("alpha beta")
                    .on_change(move |value, _| edits.lock().unwrap().push(value.to_owned())),
            );
            event
                .widgets
                .add_text_field(TextField::new(read_only, 20).editable(false));
            event.widgets.add_widget(Box::new(Custom {
                log: Log::default(),
                takes_input: Arc::new(AtomicBool::new(true)),
            }));
            assert!(event.widgets.set_initial_focus(id));
        }
    });
    with_screen_window(vec![entry].into(), |game| {
        assert!(game.mod_screen_open("widget_probe"));
        assert!(game.text_field_focused());
        game.sync_ime();
        assert!(game.ime_allowed);
        game.ime_preedit("候", Some((0, 3)));
        assert!(sunrise_client::preedit::composing());
        assert!(take(&input.events).is_empty());
        game.ime_commit("候");
        assert!(!sunrise_client::preedit::composing());
        assert_eq!(field(game).value, "alpha beta候");
        assert_eq!(take(&edits), ["alpha beta候"]);
        assert!(take(&input.events).is_empty());

        // Pre and Post get the same double-click flag, and only a double press selects a word.
        click(game, FIELD_POINT);
        assert_eq!(field(game).selected_text(), "");
        let last = game.screen_state.last_click.as_mut().unwrap();
        last.time = Instant::now();
        click(game, FIELD_POINT);
        assert_eq!(field(game).selected_text(), "alpha");
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, false),
                Input::Press("mod", 0, false, true),
                Input::Release("mod", true),
                Input::PrePress("mod", 0, true),
                Input::Press("mod", 0, true, true),
                Input::Release("mod", true),
            ]
        );
        game.ime_commit("文");
        assert_eq!(take(&edits), ["文 beta候"]);

        for point in [BUTTON_POINT, CUSTOM_POINT, OUTSIDE] {
            click(game, point);
            assert!(!game.text_field_focused());
            game.sync_ime();
            assert!(!game.ime_allowed);
            take(&input.events);
            game.ime_preedit("未", Some((0, 3)));
            assert!(!sunrise_client::preedit::composing());
            assert_eq!(take(&input.events), [Input::Preedit("mod", "未".into())]);
        }
    });
}

#[test]
#[ignore = "daily"]
fn a_panicking_widget_is_disabled_without_disabling_its_siblings() {
    if fresh_screen_case(
        module_path!(),
        "a_panicking_widget_is_disabled_without_disabling_its_siblings",
    ) {
        return;
    }
    struct PanickingCustom(Log<&'static str>);
    impl ScreenWidget for PanickingCustom {
        fn bounds(&self) -> [f32; 4] {
            CUSTOM
        }
        fn mouse_clicked(
            &mut self,
            _: &mut dyn ClientHost,
            _: [f32; 2],
            _: i32,
            _: u32,
            _: bool,
        ) -> bool {
            self.0.lock().unwrap().push("custom");
            panic!("custom exploded");
        }
    }
    let panics = Log::default();
    let refusals = Log::default();
    let entry = widget_mod(&Arc::default(), {
        let (panics, refusals) = (panics.clone(), refusals.clone());
        move |_, event| {
            if event.screen.id != "mod" {
                return;
            }
            let (button, field) = (panics.clone(), panics.clone());
            let refusals = refusals.clone();
            event
                .widgets
                .add_button(Button::new(BUTTON, Text::literal("Fail"), move |_, _| {
                    button.lock().unwrap().push("button");
                    panic!("button exploded");
                }));
            event
                .widgets
                .add_text_field(TextField::new(FIELD, 20).on_change(move |_, _| {
                    field.lock().unwrap().push("field");
                    panic!("edit exploded");
                }));
            event
                .widgets
                .add_widget(Box::new(PanickingCustom(panics.clone())));
            event.widgets.add_button(Button::new(
                [140.0, 12.0, 90.0, 20.0],
                Text::literal("Refuse"),
                move |_, host| {
                    // A real headless refusal is not a panic and must not latch.
                    let opened = ScreensClientHost::mod_screen_open(host, "unavailable");
                    refusals.lock().unwrap().push(opened);
                },
            ));
        }
    });
    with_screen_game(vec![entry].into(), |game| {
        mount_headless(game, Screen::Mod);
        assert!(game.mounted_widget_mouse_clicked(BUTTON_POINT, 0, 0, false));
        assert!(game.mounted_widget_mouse_clicked([145.0, 16.0], 0, 0, false));
        assert!(game.mounted_widget_mouse_clicked(FIELD_POINT, 0, 0, false));
        assert!(game.mounted_widget_char_typed('x', 0));
        assert!(!game.mounted_widget_mouse_clicked(CUSTOM_POINT, 0, 0, false));
        assert!(!game.mounted_widget_mouse_clicked(BUTTON_POINT, 0, 0, false));
        assert!(!game.mounted_widget_mouse_clicked(FIELD_POINT, 0, 0, false));
        assert!(!game.mounted_widget_char_typed('y', 0));
        assert!(!game.mounted_widget_mouse_clicked(CUSTOM_POINT, 0, 0, false));
        assert!(game.mounted_widget_mouse_clicked([145.0, 16.0], 0, 0, false));
        assert_eq!(take(&panics), ["button", "field", "custom"]);
        assert_eq!(take(&refusals), [false, false]);
        let notices = mod_api::__private::game::take_callback_panic_notices();
        assert_eq!(notices.len(), 3);
        for (notice, (operation, message)) in notices.iter().zip([
            ("button_press", "button exploded"),
            ("text_field_change", "edit exploded"),
            ("screen_widget_mouse_clicked", "custom exploded"),
        ]) {
            assert_eq!(notice.mod_id, "widget_probe");
            assert_eq!(notice.kind, "screen widget");
            assert_eq!(notice.operation, operation);
            assert!(notice.message.contains(message));
        }
    });
}

#[test]
#[ignore = "daily"]
fn custom_widget_receives_input_and_unhandled_events_reach_post() {
    if fresh_screen_case(
        module_path!(),
        "custom_widget_receives_input_and_unhandled_events_reach_post",
    ) {
        return;
    }
    let input = Arc::new(InputProbe::default());
    let widget_calls = Log::default();
    let takes_input = Arc::new(AtomicBool::new(true));
    let entry = widget_mod(&input, {
        let (log, takes_input) = (widget_calls.clone(), takes_input.clone());
        move |_, event| {
            if event.screen.id == "mod" {
                event.widgets.add_widget(Box::new(Custom {
                    log: log.clone(),
                    takes_input: takes_input.clone(),
                }));
            }
        }
    });
    with_screen_window(vec![entry].into(), |game| {
        let wheel = |game: &mut Game<'_>| {
            window_event(
                game,
                WindowEvent::MouseWheel {
                    device_id: DeviceId::dummy(),
                    delta: MouseScrollDelta::LineDelta(0.0, 1.0),
                    phase: TouchPhase::Moved,
                },
            )
        };
        assert!(game.mod_screen_open("widget_probe"));
        click(game, CUSTOM_POINT);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, false),
                Input::Press("mod", 0, false, true),
                Input::Release("mod", true)
            ]
        );
        assert_eq!(
            take(&widget_calls),
            [
                WidgetCall::Click(false),
                WidgetCall::Focus(true),
                WidgetCall::Release
            ]
        );
        game.screen_state.last_click.as_mut().unwrap().time = Instant::now();
        mouse(game, ElementState::Pressed, MouseButton::Left);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, true),
                Input::Press("mod", 0, true, true)
            ]
        );
        game.draw_mounted_screen_widgets(&mut HudBatch::scratch([320, 240]));
        move_pointer(game, [20.0, 61.0]);
        wheel(game);
        game.ime_commit("p");
        game.replay_line("key w down", &TestEventLoop);
        assert!(!game.keys.raw(&KeyCode::KeyW));
        let receiver = game.observed_input_screen().unwrap();
        assert!(
            game.screen_takes_first_key_release(Some(&receiver), KeyCode::KeyW)
                .unwrap()
        );
        assert!(take(&input.events).is_empty());
        assert_eq!(
            take(&widget_calls),
            [
                WidgetCall::Click(true),
                WidgetCall::Render {
                    hovered: true,
                    focused: true
                },
                WidgetCall::Drag,
                WidgetCall::Scroll,
                WidgetCall::Char('p'),
                WidgetCall::KeyDown("key.keyboard.w".into()),
                WidgetCall::KeyUp("key.keyboard.w".into()),
            ]
        );

        takes_input.store(false, Ordering::Relaxed);
        move_pointer(game, [22.0, 62.0]);
        wheel(game);
        game.ime_commit("q");
        mouse(game, ElementState::Released, MouseButton::Left);
        assert_eq!(
            take(&widget_calls),
            [
                WidgetCall::Drag,
                WidgetCall::Scroll,
                WidgetCall::Char('q'),
                WidgetCall::Release
            ]
        );
        assert_eq!(
            take(&input.events),
            [
                Input::Drag("mod"),
                Input::Scroll("mod"),
                Input::Char("mod", 'q'),
                Input::Release("mod", false)
            ]
        );

        // A vanilla receiver also consumes drag and wheel input without a mounted child.
        game.set_screen(Screen::Advancements);
        move_pointer(game, [30.0, 30.0]);
        mouse(game, ElementState::Pressed, MouseButton::Left);
        move_pointer(game, [32.0, 30.0]);
        wheel(game);
        mouse(game, ElementState::Released, MouseButton::Left);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("advancements", 0, false),
                Input::Press("advancements", 0, false, true),
                Input::Release("advancements", true),
            ]
        );

        // A vanilla text field similarly suppresses character and preedit Post.
        game.begin_chat_screen("", false);
        game.ime_preedit("案", Some((0, 3)));
        assert!(sunrise_client::preedit::composing());
        game.ime_commit("案");
        assert!(take(&input.events).is_empty());
    });
}

#[test]
#[ignore = "daily"]
fn mouse_press_post_can_override_the_handled_result_for_double_clicks() {
    if fresh_screen_case(
        module_path!(),
        "mouse_press_post_can_override_the_handled_result_for_double_clicks",
    ) {
        return;
    }
    let input = Arc::new(InputProbe::default());
    let clicks = Log::default();
    let entry = widget_mod(&input, {
        let clicks = clicks.clone();
        move |_, event| {
            if event.screen.id == "mod" {
                let clicks = clicks.clone();
                event.widgets.add_button(Button::new(
                    BUTTON,
                    Text::literal("Probe"),
                    move |_, _| {
                        clicks.lock().unwrap().push(());
                    },
                ));
            }
        }
    });
    with_screen_window(vec![entry].into(), |game| {
        assert!(game.mod_screen_open("widget_probe"));
        // An unhandled click becomes handled when Post claims it.
        *input.press_result.lock().unwrap() = Some(true);
        click(game, OUTSIDE);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, false),
                Input::Press("mod", 0, false, false),
                Input::Release("mod", false)
            ]
        );
        assert_eq!(game.screen_state.last_click.unwrap().button, 0);

        // A cancelled Pre suppresses both the mounted callback and Post, even if
        // a listener would otherwise claim the click.
        game.screen_state.last_click = None;
        input.cancel_press.store(true, Ordering::Relaxed);
        click(game, BUTTON_POINT);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, false),
                Input::Release("mod", false)
            ]
        );
        assert!(take(&clicks).is_empty());

        // The reverse override must apply too, even when the callback already ran.
        game.screen_state.last_click = None;
        input.cancel_press.store(false, Ordering::Relaxed);
        *input.press_result.lock().unwrap() = Some(false);
        click(game, BUTTON_POINT);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, false),
                Input::Press("mod", 0, false, true),
                Input::Release("mod", false)
            ]
        );
        assert_eq!(take(&clicks), [()]);
        assert!(game.screen_state.last_click.is_none());
        click(game, BUTTON_POINT);
        assert_eq!(
            take(&input.events),
            [
                Input::PrePress("mod", 0, false),
                Input::Press("mod", 0, false, true),
                Input::Release("mod", false)
            ]
        );
    });
}

#[test]
#[ignore = "daily"]
fn unhandled_key_posts_can_stop_the_mapping_on_both_edges() {
    if fresh_screen_case(
        module_path!(),
        "unhandled_key_posts_can_stop_the_mapping_on_both_edges",
    ) {
        return;
    }
    let input = Arc::new(InputProbe::default());
    let entry = widget_mod(&input, |_, _| {});
    with_screen_window(vec![entry].into(), |game| {
        const KEY: KeyCode = KeyCode::KeyW;
        assert!(game.mod_screen_open("widget_probe"));
        // Use the actual control-socket press path, then the final physical-key
        // dispatch used by WindowEvent::KeyboardInput (winit cannot fabricate a KeyEvent).
        input.cancel_key.store(true, Ordering::Relaxed);
        game.replay_line("key w down", &TestEventLoop);
        assert!(!game.keys.raw(&KEY));
        assert_eq!(
            take(&input.events),
            [Input::KeyDown("mod", "key.keyboard.w".into())]
        );
        game.replay_line("key w up", &TestEventLoop);

        let receiver = game.observed_input_screen().unwrap();
        game.set_screen(Screen::Paused);
        game.finish_window_key_event(Some(&receiver), KEY, true, false)
            .unwrap();
        assert!(!game.keys.raw(&KEY));
        assert_eq!(
            take(&input.events),
            [Input::KeyDown("mod", "key.keyboard.w".into())]
        );

        input.cancel_key.store(false, Ordering::Relaxed);
        game.finish_window_key_event(Some(&receiver), KEY, true, false)
            .unwrap();
        assert!(game.keys.raw(&KEY));
        assert_eq!(
            take(&input.events),
            [Input::KeyDown("mod", "key.keyboard.w".into())]
        );

        input.cancel_key.store(true, Ordering::Relaxed);
        assert!(
            !game
                .screen_takes_first_key_release(Some(&receiver), KEY)
                .unwrap()
        );
        game.finish_window_key_event(Some(&receiver), KEY, false, false)
            .unwrap();
        assert!(game.keys.raw(&KEY));
        assert_eq!(
            take(&input.events),
            [Input::KeyUp("mod", "key.keyboard.w".into())]
        );
        input.cancel_key.store(false, Ordering::Relaxed);
        game.finish_window_key_event(Some(&receiver), KEY, false, false)
            .unwrap();
        assert!(!game.keys.raw(&KEY));
        assert_eq!(
            take(&input.events),
            [Input::KeyUp("mod", "key.keyboard.w".into())]
        );
    });
}
