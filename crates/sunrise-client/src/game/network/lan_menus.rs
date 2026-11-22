//! The menus a LAN guest has open that the game runs for them, kept outside the guest's
//! `GuestPlay`. A guest's turn (`as_guest`) lifts the whole `GuestPlay` out of the host while it
//! runs, so a menu owner kept there is out of reach of anything that runs inside the turn: another
//! nested turn, a hook, an action that needs to find the menu to admit or undo. These owners stay
//! in the host, by connection, for as long as the guest is here.
use std::collections::VecDeque;

use super::{
    lan::LanHost, lan_crafting::GuestCrafting, lan_stations::GuestStation,
    transactions::decision::MenuOwner,
};

/// One guest's menu owners: the inventory menu, the crafting table and the furnace or brewing
/// stand (`GuestCrafting`), and the workstation. Each owner counts every access, so an action can
/// tell that its menu was touched.
#[derive(Default)]
pub(in crate::game) struct GuestMenus {
    /// The inventory menu, the crafting table and the machine menu.
    pub(in crate::game) crafting: GuestCrafting,
    /// The workstation menu the guest has open (game/network/lan_stations.rs).
    pub(in crate::game) station: MenuOwner<Option<GuestStation>>,
    /// The transient menu engine, kept between clicks for drag state.
    pub(super) owned_menu: Option<(u64, Box<sunrise_client::menu::Menu>)>,
    /// Identity and mutation stamp for this connection's merchant window, even while loaned.
    pub(super) merchant: MenuOwner<()>,
    /// The outer `menu_clicked` decision. Nested default routing can decide a pickup on `hub`.
    pub(super) click: MenuOwner<()>,
    /// Names the hub's container clicks of this guest, which have no menu value of their own: a
    /// click while another of the guest's is being decided is refused as busy.
    pub(in crate::game) hub: MenuOwner<()>,
    /// The successful open already announced to mods, retained through a loan of `GuestPlay`.
    pub(in crate::game) menu_heard: Option<super::native_menus::HeardMenu>,
    /// Installed from the start of `removed` until the general Close returns.
    pub(super) closing: bool,
    /// The storage a mod backed its open menu's slots with (`player_back_mod_menu_slots`);
    /// None for every menu no mod backed.
    pub(in crate::game) slot_places: Option<super::native_menu_places::MenuSlotPlaces>,
}

pub(super) type MenuOpen = Box<dyn FnOnce(&mut super::Game<'_>) + Send>;

/// One reservation is also the identity used to make its publication ready.
#[derive(Clone, Copy)]
pub(super) struct MenuReservation {
    pub(super) conn: u32,
    pub(super) window: i32,
    pub(super) generation: u64,
}

impl MenuReservation {
    pub(super) fn identity(self) -> (i32, u64) {
        (self.window, self.generation)
    }
}

struct QueuedOpen {
    reservation: MenuReservation,
    ready: Option<(MenuOpen, bool)>,
}

/// The publication stream for one connection. Root-commit opens reserve their places
/// before any listeners run; closes and guest turns may delay publication, not reorder it.
#[derive(Default)]
pub(super) struct MenuPublications {
    queue: VecDeque<QueuedOpen>,
    holding: usize,
    replacing_window: Option<i32>,
    publishing: bool,
}

/// A close or travel still owns publication until the original replacement is installed.
pub(super) struct MenuOpenHold(Option<i32>);

/// An authoritative click reply, published only once the click's transaction has committed.
pub(super) struct MenuClickReply {
    conn: u32,
    generation: u64,
    reply: ClickReply,
}

pub(super) enum ClickReply {
    Predicted(sunrise_protocol::packets::play::ContainerClick, Vec<usize>),
    Resend(i32),
    Close(i32),
    ClientClose(i32),
    Inventory,
    Offhand,
}

impl super::Game<'_> {
    pub(super) fn reply_predicted_menu_click(
        &mut self,
        conn: u32,
        packet: &sunrise_protocol::packets::play::ContainerClick,
        changed: Vec<usize>,
    ) {
        if self.transactions.is_empty() {
            if let Some(host) = self.lan.as_mut() {
                host.hub
                    .inventory_clicked(conn, packet, &self.world, &changed);
            }
        } else {
            self.reply_menu_click(conn, ClickReply::Predicted(packet.clone(), changed));
        }
    }

    pub(super) fn reply_menu_click_for(&mut self, conn: u32, generation: u64, reply: ClickReply) {
        self.after_commit(super::transactions::After::MenuClick(Box::new(
            MenuClickReply {
                conn,
                generation,
                reply,
            },
        )));
    }

    pub(super) fn reply_menu_click(&mut self, conn: u32, reply: ClickReply) {
        if let Some(guest) = self.lan.as_ref().and_then(|host| host.hub.guest(conn)) {
            self.reply_menu_click_for(conn, guest.menu_generation, reply);
        }
    }

    /// Whether a mod's open must queue rather than install immediately.
    pub(super) fn menu_open_waits(&self, conn: u32) -> bool {
        !self.transactions.is_empty() || self.transactions.menu_publications.contains_key(&conn)
    }

    pub(super) fn last_menu_reservation(&self, conn: u32) -> Option<i32> {
        let stream = self.transactions.menu_publications.get(&conn)?;
        stream
            .queue
            .back()
            .map(|next| next.reservation.window)
            .or(stream.replacing_window)
    }

    pub(super) fn hold_menu_opens(&mut self, conn: u32, replacing: Option<i32>) -> MenuOpenHold {
        let stream = self.transactions.menu_publications.entry(conn).or_default();
        stream.holding += 1;
        let previous = stream.replacing_window;
        stream.replacing_window = replacing.or(previous);
        MenuOpenHold(previous)
    }

    pub(super) fn resume_menu_opens(&mut self, conn: u32, hold: MenuOpenHold) {
        let stream = self.transactions.menu_publications.get_mut(&conn).unwrap();
        stream.holding -= 1;
        stream.replacing_window = hold.0;
        self.drain_menu_opens(conn);
    }

    pub(super) fn queue_menu_reservation(&mut self, reservation: MenuReservation) {
        self.transactions
            .menu_publications
            .entry(reservation.conn)
            .or_default()
            .queue
            .push_back(QueuedOpen {
                reservation,
                ready: None,
            });
    }

    pub(super) fn publish_menu_open(
        &mut self,
        reservation: MenuReservation,
        after_turn: bool,
        open: MenuOpen,
    ) {
        let conn = reservation.conn;
        let queue = &mut self
            .transactions
            .menu_publications
            .entry(conn)
            .or_default()
            .queue;
        let ready = Some((open, after_turn));
        if let Some(entry) = queue
            .iter_mut()
            .find(|e| e.reservation.generation == reservation.generation)
        {
            entry.ready = ready;
        } else {
            queue.push_back(QueuedOpen { reservation, ready });
        }
        self.drain_menu_opens(conn);
    }

    pub(super) fn drain_menu_opens(&mut self, conn: u32) {
        let Some(stream) = self.transactions.menu_publications.get_mut(&conn) else {
            return;
        };
        if stream.holding != 0 || stream.publishing {
            return;
        }
        stream.publishing = true;
        loop {
            let stream = self.transactions.menu_publications.get_mut(&conn).unwrap();
            let Some((_, after_turn)) = stream.queue.front().and_then(|head| head.ready.as_ref())
            else {
                break;
            };
            if *after_turn && self.lan_guest_swaps.contains(&conn) {
                break;
            }
            let next = stream.queue.pop_front().unwrap();
            stream.replacing_window = Some(next.reservation.window);
            let (open, _) = next.ready.unwrap();
            open(self);
        }
        let stream = self.transactions.menu_publications.get_mut(&conn).unwrap();
        stream.publishing = false;
        stream.replacing_window = None;
        if stream.queue.is_empty() {
            self.transactions.menu_publications.remove(&conn);
        }
    }

    pub(super) fn publish_menu_click(&mut self, reply: MenuClickReply) {
        use sunrise_protocol::packets::play::{Clientbound as C, SetPlayerInventory};
        let MenuClickReply {
            conn,
            generation,
            reply,
        } = reply;
        if let ClickReply::Close(window) | ClickReply::ClientClose(window) = reply {
            self.close_guest_window(
                conn,
                window,
                generation,
                matches!(reply, ClickReply::Close(_)),
            );
            return;
        }
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let Some(i) = host.hub.guests.connection_index(conn) else {
            return;
        };
        let guest = &host.hub.guests[i];
        if guest.menu_generation != generation || guest.disconnect_pending {
            return;
        }
        match reply {
            ClickReply::Predicted(packet, changed) => {
                let window = host.hub.guests[i].menu.as_ref().map_or(0, |m| m.window);
                if packet.container == window {
                    host.hub
                        .inventory_clicked(conn, &packet, &self.world, &changed);
                }
            }
            ClickReply::Resend(window) => host.hub.resend_menu(conn, window, &self.world),
            ClickReply::Inventory => host.hub.send_inventory(i),
            ClickReply::Offhand => {
                let offhand = guest.offhand.clone();
                let item = host.hub.stack(&offhand);
                host.hub.guests.update(i, |g| {
                    g.send(&C::SetPlayerInventory(SetPlayerInventory {
                        slot: 40,
                        item,
                    }));
                    g.menu_sent[45] = offhand;
                });
            }
            ClickReply::Close(_) | ClickReply::ClientClose(_) => unreachable!(),
        }
    }
}

impl LanHost {
    /// Forget `conn`'s menu owners: the guest left or was removed, so nothing can name them again.
    pub(in crate::game) fn forget_menus(&mut self, conn: u32) -> Option<GuestMenus> {
        self.menus.remove(&conn)
    }
}
