//! Private custom-book connection state and client network services.
use super::Game;
#[path = "native_net/packet_hold.rs"]
mod packet_hold;
use mod_api::{SendPacketError, ServerboundPlayPacket};
use std::collections::HashMap;
#[derive(Default)]
pub(super) struct NativeNet {
    pub(super) holds: std::cell::RefCell<packet_hold::Holds>,
    pub(super) placement: super::native_book_placement::State,
    pub(super) book_connections:
        HashMap<u32, sunrise_client::recipe_book::settings_connection::Connection>,
    pub(super) book_view: sunrise_client::recipe_book::settings_sync::ClientView,
    pub(super) book_request: u64,
    pub(super) book_ready: bool,
    pub(super) book_retry_at: Option<std::time::Instant>,
    /// A mod's `disconnect` (its reason, if any), carried out at the end of the frame.
    pub(super) leave: Option<super::native_disconnect::LeaveRequest>,
}

impl Game<'_> {
    /// Opens the chat screen with `text` typed in and the cursor at its end,
    /// as T and / open it (`ChatScreen` with its initial text): a command
    /// macro can fill in `/tp ` for the player to finish. Nothing is sent; the
    /// player edits and sends it, or presses Escape. The box keeps its limit
    /// (256 UTF-16 units) and its character filter. The ordinary screen
    /// opening and closing hooks govern the transition. API @API:open_chat@.
    pub(super) fn open_chat(&mut self, text: &str) -> Result<(), String> {
        if self.begin_chat_screen(text, false) {
            Ok(())
        } else {
            Err("no player in a world".into())
        }
    }

    /// Validate and reserve a vanilla packet for root-commit publication on this
    /// exact physical protocol incarnation. Registered and engine-reserved
    /// channels require their payload/service API; unrelated plugin payloads
    /// retain vanilla's discard-codec limit. No local effects are applied. API 409.
    pub(super) fn send_packet(
        &mut self,
        packet: ServerboundPlayPacket,
    ) -> Result<(), SendPacketError> {
        let remote = self.remote.as_ref().ok_or(SendPacketError::NotConnected)?;
        if !remote.joined || remote.reconfiguring.is_some() {
            return Err(SendPacketError::NotInPlay);
        }
        let body = packet.clone();
        let prepared = remote.session.prepare_mod_packet(packet)?;
        self.after_commit(super::transactions::After::Packet(prepared, body));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::{run_fresh_cases, with_client_game};
    use super::{SendPacketError, ServerboundPlayPacket};
    use mod_api::Identifier;
    use sunrise_core::vanilla_bytes::Writer;
    use sunrise_protocol::connection::Pipe;

    #[test]
    #[ignore = "daily"]
    fn open_chat_replaces_screens_but_a_veto_keeps_the_old_chat() {
        use crate::game::Screen;
        use mod_api::{
            ClientChatEvent, ClientHost, Flow, Hooks, ScreenClosingEvent, ScreenInitPostEvent,
            ScreenOpening, ScreenOpeningEvent,
        };
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        use sunrise_client::motion::PlayerMotion;
        use sunrise_core::geometry::Vec3;
        use sunrise_protocol::packets::play::Serverbound;

        if run_fresh_cases(
            "game::native_net::tests::open_chat_replaces_screens_but_a_veto_keeps_the_old_chat",
            "SUNRISE_OPEN_CHAT_CASE",
            &["transition"],
        ) {
            return;
        }
        #[derive(Default)]
        struct Probe {
            cancel: AtomicBool,
            events: Mutex<Vec<String>>,
        }
        struct Listener(Arc<Probe>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn screen_opening(
                &self,
                _: &dyn ClientHost,
                event: &mut ScreenOpeningEvent<'_>,
            ) -> Option<ScreenOpening> {
                self.0
                    .events
                    .lock()
                    .unwrap()
                    .push(format!("{}->{}", event.current.id, event.new.id));
                self.0
                    .cancel
                    .load(Ordering::Relaxed)
                    .then_some(ScreenOpening::Cancel)
            }

            fn screen_closing(&self, _: &dyn ClientHost, event: &mut ScreenClosingEvent<'_>) {
                self.0
                    .events
                    .lock()
                    .unwrap()
                    .push(format!("close {}", event.screen.id));
            }

            fn screen_init_post(&self, _: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_>) {
                self.0
                    .events
                    .lock()
                    .unwrap()
                    .push(format!("init {}", event.screen.id));
            }

            fn client_chat(&self, host: &mut dyn ClientHost, _: &mut ClientChatEvent<'_>) -> Flow {
                host.open_chat("replacement on submit").unwrap();
                Flow::Continue
            }
        }
        let probe = Arc::new(Probe::default());
        let mods = vec![mod_api::ModEntry::new(
            "chat_screens",
            Box::new(Listener(probe.clone())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_client_game(|game| {
                game.player = None;
                assert!(game.open_chat("/tp ").is_err());
                assert!(game.ui.chat_screen.is_none());
                let (client, _server) = Pipe::pair();
                game.connect_local(client, "ChatTest").unwrap();
                let (session, mut peer, _keepalive) =
                    sunrise_client::net::Session::packet_hold_probe();
                let remote = game.remote.as_mut().unwrap();
                remote.joined = true;
                remote.session = session;
                game.player =
                    Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
                game.screen = Screen::Paused;
                game.send_unattended_command("unknown_command");
                assert!(game.command_prompt.is_some());
                probe.events.lock().unwrap().clear();

                probe.cancel.store(true, Ordering::Relaxed);
                assert_eq!(game.open_chat("vetoed"), Ok(()));
                assert!(game.command_prompt.is_some());
                assert!(game.ui.chat_screen.is_none());
                game.observe_screen();
                assert_eq!(
                    std::mem::take(&mut *probe.events.lock().unwrap()),
                    ["command_prompt->chat"]
                );

                probe.cancel.store(false, Ordering::Relaxed);
                assert_eq!(game.open_chat("/tp "), Ok(()));
                assert!(game.command_prompt.is_none());
                assert!(game.screen == Screen::Chat);
                assert_eq!(game.ui.chat_screen.as_ref().unwrap().value(), "/tp ");
                assert!(!game.screen_over_playing());
                assert_eq!(game.open_chat("second draft"), Ok(()));
                assert_eq!(
                    game.ui.chat_screen.as_ref().unwrap().value(),
                    "second draft"
                );
                assert_eq!(game.ui.chat_draft.as_ref().unwrap().text, "/tp ");
                game.observe_screen();
                assert_eq!(
                    *probe.events.lock().unwrap(),
                    [
                        "command_prompt->chat",
                        "close command_prompt",
                        "init chat",
                        "chat->chat",
                        "close chat",
                        "init chat",
                    ]
                );
                peer.reader
                    .set_read_timeout(Some(std::time::Duration::from_millis(50)))
                    .unwrap();
                assert!(
                    peer.recv().unwrap_err().is_timed_out(),
                    "opening neither answers the prompt nor sends the prefill"
                );
                peer.reader
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                game.chat_key_code(winit::keyboard::KeyCode::Enter, None);
                let (id, body) = peer.recv().unwrap();
                let Serverbound::Chat(chat) = Serverbound::decode(id, &body).unwrap() else {
                    panic!("expected the submitted chat line");
                };
                assert_eq!(chat.message, "second draft");
                assert_eq!(
                    game.ui.chat_screen.as_ref().unwrap().value(),
                    "replacement on submit",
                    "the callback's new chat screen must not close with the submitted one"
                );
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn waking_with_typed_chat_replaces_it_once_without_saving_a_draft() {
        use crate::game::Screen;
        use mod_api::{
            ClientHost, Hooks, ScreenClosingEvent, ScreenInitPostEvent, ScreenOpening,
            ScreenOpeningEvent,
        };
        use std::sync::{Arc, Mutex};
        use sunrise_client::motion::PlayerMotion;
        use sunrise_core::geometry::Vec3;

        if run_fresh_cases(
            "game::native_net::tests::waking_with_typed_chat_replaces_it_once_without_saving_a_draft",
            "SUNRISE_WAKE_CHAT_CASE",
            &["wake"],
        ) {
            return;
        }
        struct Listener(Arc<Mutex<Vec<String>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn screen_opening(
                &self,
                _: &dyn ClientHost,
                event: &mut ScreenOpeningEvent<'_>,
            ) -> Option<ScreenOpening> {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("{}->{}", event.current.id, event.new.id));
                None
            }
            fn screen_closing(&self, _: &dyn ClientHost, event: &mut ScreenClosingEvent<'_>) {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("close {}", event.screen.id));
            }
            fn screen_init_post(&self, _: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_>) {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("init {}", event.screen.id));
            }
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        mod_platform::testing::with_mods(
            vec![mod_api::ModEntry::new(
                "wake_chat",
                Box::new(Listener(events.clone())),
            )]
            .into(),
            || {
                with_client_game(|game| {
                    game.player =
                        Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
                    // The tick following wake-up: the existing bed chat still has typed input.
                    assert!(game.begin_chat_screen("typed in bed", false));
                    game.ui.in_bed_chat = true;
                    events.lock().unwrap().clear();
                    game.tick_in_bed_chat();
                    game.observe_screen();
                    assert!(game.screen == Screen::Chat);
                    assert!(!game.ui.in_bed_chat);
                    assert_eq!(
                        game.ui.chat_screen.as_ref().unwrap().value(),
                        "typed in bed"
                    );
                    assert!(game.ui.chat_draft.is_none());
                    assert_eq!(
                        *events.lock().unwrap(),
                        ["chat->chat", "close chat", "init chat"]
                    );
                });
            },
        );
    }

    /// A joined flag cannot manufacture a physical transport. Packet codec,
    /// reservation, and framing cases live in net::mod_packet_tests.
    #[test]
    #[ignore = "daily"]
    fn mod_packets_require_joined_player_and_physical_transport() {
        if run_fresh_cases(
            "game::native_net::tests::mod_packets_require_joined_player_and_physical_transport",
            "SUNRISE_PACKET_GUARD_CHILD",
            &["guards"],
        ) {
            return;
        }
        with_client_game(|game| {
            let packet = || {
                let mut body = Writer::new();
                body.string("voicechat:request_secret");
                body.bytes(&[1, 2, 3, 4]);
                ServerboundPlayPacket::new(
                    Identifier::parse("minecraft:custom_payload").unwrap(),
                    body.data,
                )
            };
            assert_eq!(
                game.send_packet(packet()),
                Err(SendPacketError::NotConnected)
            );
            let (client, _server) = Pipe::pair();
            game.connect_local(client, "PacketTest").unwrap();
            assert_eq!(game.send_packet(packet()), Err(SendPacketError::NotInPlay));
            // Deliberately inconsistent actor state: no handshake has installed
            // a writer. Acceptance must still require the physical provider.
            game.remote.as_mut().unwrap().joined = true;
            assert_eq!(
                game.send_packet(packet()),
                Err(SendPacketError::NotConnected)
            );
        });
    }
}
