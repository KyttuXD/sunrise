//! Mod-owned barriers over one ordered queue for the current physical play incarnation.
use super::{Game, PlaySend};
use mod_api::{HoldError, HoldId, SendPacketError, content::crash::current_native_mod};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use sunrise_client::net::PreparedPacket;
use sunrise_protocol::packets::play::Serverbound as S;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Hold {
    id: HoldId,
    owner: &'static str,
    start: u64,
}

pub(in crate::game) struct Holds<P = PreparedPacket> {
    incarnation: Option<(u64, u64)>,
    active: Vec<Hold>,
    // Each packet is tagged with the most recently issued hold, even if that
    // hold has since ended. The tags never go backwards within the queue.
    latest: u64,
    packets: VecDeque<(u64, P)>,
}

impl<P> Default for Holds<P> {
    fn default() -> Self {
        Self {
            incarnation: None,
            active: Vec::new(),
            latest: 0,
            packets: VecDeque::new(),
        }
    }
}

impl<P> Holds<P> {
    pub(in crate::game) fn clear(&mut self) {
        *self = Self::default();
    }

    fn sync(&mut self, incarnation: Option<(u64, u64)>) {
        if self.incarnation != incarnation {
            self.clear();
            self.incarnation = incarnation;
        }
    }

    fn start(&mut self, owner: &'static str) -> Result<HoldId, HoldError> {
        if self.incarnation.is_none() {
            return Err(HoldError::NoWorld);
        }
        let start = NEXT.fetch_add(1, Ordering::Relaxed);
        let id = HoldId::new(start);
        self.active.push(Hold { id, owner, start });
        self.latest = start;
        Ok(id)
    }

    fn end(&mut self, owner: &'static str, id: HoldId) -> Result<Hold, HoldError> {
        let index = self
            .active
            .iter()
            .position(|hold| hold.id == id)
            .ok_or(HoldError::Unknown)?;
        if self.active[index].owner != owner {
            return Err(HoldError::WrongOwner);
        }
        Ok(self.active.remove(index))
    }

    fn queue(&mut self, packet: P) {
        self.packets.push_back((self.latest, packet));
    }

    fn release(
        &mut self,
        owner: &'static str,
        id: HoldId,
        mut publish: impl FnMut(P) -> Result<(), SendPacketError>,
    ) -> Result<usize, HoldError> {
        self.end(owner, id)?;
        let ready = self.active.first().map_or(self.packets.len(), |hold| {
            self.packets.partition_point(|(tag, _)| *tag < hold.start)
        });
        let result = self
            .packets
            .drain(..ready)
            .enumerate()
            .try_for_each(|(sent, (_, packet))| {
                publish(packet).map_err(|error| HoldError::Send { sent, error })
            });
        if matches!(
            result,
            Err(HoldError::Send {
                error: SendPacketError::NotInPlay | SendPacketError::NotConnected,
                ..
            })
        ) {
            self.clear();
        }
        result.map(|()| ready)
    }

    fn discard(&mut self, owner: &'static str, id: HoldId) -> Result<(), HoldError> {
        let start = self.end(owner, id)?.start;
        // This hold was active for every packet admitted from `start` onward,
        // including overlapping packets. Older packets and other leases stay put.
        let keep = self.packets.partition_point(|(tag, _)| *tag < start);
        self.packets.truncate(keep);
        Ok(())
    }
}

impl Game<'_> {
    fn hold_incarnation(&self) -> Option<(u64, u64)> {
        self.remote
            .as_ref()
            .filter(|r| r.joined && r.reconfiguring.is_none())?
            .session
            .hold_incarnation()
    }

    pub(in crate::game) fn hold_packets(&mut self) -> Result<HoldId, HoldError> {
        let owner = current_native_mod().ok_or(HoldError::NoOwner)?;
        let mut holds = self.native_net.holds.borrow_mut();
        holds.sync(self.hold_incarnation());
        holds.start(owner)
    }

    pub(in crate::game) fn release_held(&mut self, hold: HoldId) -> Result<usize, HoldError> {
        let owner = current_native_mod().ok_or(HoldError::NoOwner)?;
        let mut holds = self.native_net.holds.borrow_mut();
        holds.sync(self.hold_incarnation());
        holds.release(owner, hold, PreparedPacket::try_publish)
    }

    pub(in crate::game) fn drop_held(&mut self, hold: HoldId) -> Result<(), HoldError> {
        let owner = current_native_mod().ok_or(HoldError::NoOwner)?;
        let mut holds = self.native_net.holds.borrow_mut();
        holds.sync(self.hold_incarnation());
        holds.discard(owner, hold)
    }

    pub(in crate::game) fn publish_mod_packet(
        &self,
        packet: PreparedPacket,
        owner: Option<&'static str>,
    ) {
        let mut holds = self.native_net.holds.borrow_mut();
        if !holds.active.is_empty() {
            holds.sync(self.hold_incarnation());
            if !holds.active.is_empty() && holds.incarnation == Some(packet.incarnation()) {
                holds.queue(packet);
                return;
            }
        }
        let result = if self
            .remote
            .as_ref()
            .is_some_and(|remote| remote.session.serial() == packet.incarnation().0)
        {
            packet.try_publish()
        } else {
            Err(SendPacketError::NotConnected)
        };
        if let Err(error) = result {
            let owner = owner.unwrap_or("game");
            log::warn!("{owner}: send_packet publication refused: {error:?}");
        }
    }

    /// Run `client_packet_send`, then admit an uncancelled play packet to the
    /// hold queue or the writer. Admission does not promise delivery.
    pub(in crate::game) fn send_play(&self, packet: S) -> PlaySend {
        let Some(remote) = &self.remote else {
            return PlaySend::Refused;
        };
        if self.client_packet_send_canceled(&packet) {
            return PlaySend::Cancelled;
        }
        let mut holds = self.native_net.holds.borrow_mut();
        let result = if !holds.active.is_empty()
            && let Some(incarnation) = holds.incarnation
        {
            remote
                .session
                .prepare_held_play(packet, incarnation)
                .map(|packet| holds.queue(packet))
        } else {
            remote.session.send(packet)
        };
        match result {
            Ok(()) => PlaySend::Admitted,
            Err(error) => {
                log::warn!("play packet refused: {error:?}");
                if !holds.active.is_empty() {
                    holds.sync(self.hold_incarnation());
                }
                PlaySend::Refused
            }
        }
    }
}

#[cfg(test)]
#[path = "packet_hold_tests.rs"]
mod tests;
