//! The menus a LAN guest has open that the game runs for them, kept outside the guest's
//! `GuestPlay`. A guest's turn (`as_guest`) lifts the whole `GuestPlay` out of the host while it
//! runs, so a menu owner kept there is out of reach of anything that runs inside the turn: another
//! nested turn, a hook, an action that needs to find the menu to admit or undo. These owners stay
//! in the host, by connection, for as long as the guest is here.
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
    /// Names the hub's container clicks of this guest, which have no menu value of their own: a
    /// click while another of the guest's is being decided is refused as busy.
    pub(in crate::game) hub: MenuOwner<()>,
    /// The successful open already announced to mods, retained through a loan of `GuestPlay`.
    pub(in crate::game) menu_heard: Option<super::native_menus::HeardMenu>,
    /// Installed from the start of `removed` until the general Close returns.
    pub(super) closing: Option<ClosingMenu>,
    /// The storage a mod backed its open menu's slots with (`player_back_mod_menu_slots`);
    /// None for every menu no mod backed.
    pub(in crate::game) slot_places: Option<super::native_menu_places::MenuSlotPlaces>,
}

#[derive(Default)]
pub(super) struct ClosingMenu {
    pub(super) opens: Vec<Box<super::native_menus::PendingMenuOpen>>,
}

/// An authoritative click reply, published only once the click's transaction has committed.
pub(super) struct MenuClickReply {
    conn: u32,
    generation: u64,
    reply: ClickReply,
}

impl MenuClickReply {
    pub(super) fn closes_menu(&self) -> bool {
        matches!(
            self.reply,
            ClickReply::Close(_) | ClickReply::ClientClose(_)
        )
    }
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
    /// The addressed player's menu owns its level. Run its lifecycle there, even when
    /// a callback from another level requested it, and restore the caller's level.
    pub(super) fn in_menu_owner_level<R>(
        &mut self,
        conn: u32,
        run: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let guest = self.lan.as_ref()?.hub.guest(conn)?;
        let dim = super::Dim::from_id(&guest.dimension);
        if dim == self.dim {
            return Some(run(self));
        }
        let back = self.enter_command_level(dim)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self)));
        self.enter_command_level(back);
        match result {
            Ok(value) => Some(value),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

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

    /// Run `removed` only after the outermost commit. An abort keeps the current menu.
    pub(super) fn request_menu_close(&mut self, conn: u32, window: i32, generation: u64) {
        self.reply_menu_click_for(conn, generation, ClickReply::Close(window));
    }

    /// An open waits for an enclosing transaction or for the old menu to finish closing.
    pub(super) fn menu_open_waits(&self, conn: u32) -> bool {
        !self.transactions.is_empty()
            || self
                .lan
                .as_ref()
                .and_then(|host| host.menus.get(&conn))
                .is_some_and(|menus| menus.closing.is_some())
    }

    pub(super) fn defer_menu_open(
        &mut self,
        conn: u32,
        open: super::native_menus::PendingMenuOpen,
    ) -> i32 {
        let staged = self.transactions.next_staged_menu_open();
        self.after_commit(super::transactions::After::MenuOpen(conn, Box::new(open)));
        staged
    }

    pub(super) fn publish_menu_open(
        &mut self,
        conn: u32,
        open: super::native_menus::PendingMenuOpen,
    ) {
        if let Some(closing) = self
            .lan
            .as_mut()
            .and_then(|host| host.menus.get_mut(&conn))
            .and_then(|menus| menus.closing.as_mut())
        {
            closing.opens.push(Box::new(open));
        } else {
            self.install_pending_menu_open(conn, open);
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
