//! Checked publication for the client actor's ordered hold queue.
use super::*;
use mod_api::SendPacketError;

impl Session {
    /// Queue a play packet, reporting admission rather than delivery. No play
    /// packets enter the writer in configuration (`handleConfigurationStart`).
    pub fn send(&self, packet: play::Serverbound) -> Result<(), SendPacketError> {
        let body = self.encode_engine_play(packet)?;
        let writer = self.writer.lock().unwrap();
        if self.configuring.load(Ordering::Relaxed) {
            return Err(SendPacketError::NotInPlay);
        }
        writer
            .as_ref()
            .ok_or(SendPacketError::NotConnected)?
            .send_engine_play(body)
    }

    fn encode_engine_play(&self, packet: play::Serverbound) -> Result<Vec<u8>, SendPacketError> {
        if self.debug
            && !matches!(
                packet,
                play::Serverbound::ClientTickEnd(_)
                    | play::Serverbound::MovePlayerPos(_)
                    | play::Serverbound::MovePlayerPosRot(_)
            )
        {
            eprintln!("[net] -> {packet:?}");
        }
        packet
            .try_encode()
            .map_err(|error| SendPacketError::EncodeFailed {
                message: error.to_string(),
            })
    }

    /// Physical connection and play incarnation; changes even after Play/CFG/Play.
    pub fn hold_incarnation(&self) -> Option<(u64, u64)> {
        let writer = self.writer.lock().unwrap();
        if self.configuring.load(Ordering::Relaxed) {
            return None;
        }
        writer
            .as_ref()?
            .play_incarnation()
            .map(|generation| (self.serial(), generation))
    }
    /// Reserve an engine send against the same physical writer as other play traffic.
    pub fn prepare_held_play(
        &self,
        packet: play::Serverbound,
        incarnation: (u64, u64),
    ) -> Result<PreparedPacket, SendPacketError> {
        let (serial, generation) = incarnation;
        if serial != self.serial() {
            return Err(SendPacketError::NotConnected);
        }
        let body = self.encode_engine_play(packet)?;
        let writer = self.writer.lock().unwrap();
        if self.configuring.load(Ordering::Relaxed) {
            return Err(SendPacketError::NotInPlay);
        }
        let writer = writer.as_ref().ok_or(SendPacketError::NotConnected)?;
        let prepared = writer.prepare_packet(body, serial, Some(generation));
        match &prepared {
            // Match the ordinary writer's terminal behavior: pressure closes immediately,
            // while a framing error follows the packets already published to the FIFO.
            Err(SendPacketError::Backpressure) => writer.shutdown(),
            Err(SendPacketError::FrameTooLarge { .. } | SendPacketError::EncodeFailed { .. }) => {
                writer.disconnect(None);
            }
            _ => {}
        }
        prepared
    }
}

#[cfg(feature = "test-payload-probe")]
impl Session {
    /// Physical play transport for actor tests, plus the network thread's direct reply path.
    #[doc(hidden)]
    pub fn packet_hold_probe() -> (Self, Connection, impl Fn(i64)) {
        let (client, server) = sunrise_protocol::connection::Pipe::pair();
        let (_, physical) = Connection::local(client).split();
        let writer = QueuedWriter::new(physical).unwrap();
        writer
            .transition_protocol(sunrise_protocol::payload_negotiation::Phase::Play, &[3])
            .unwrap();
        let mut peer = Connection::local(server);
        peer.reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(peer.recv().unwrap(), (3, vec![]));
        let session = Self::replay("hold probe");
        *session.writer.lock().unwrap() = Some(writer.clone());
        (session, peer, move |id| {
            writer
                .send(&play::Serverbound::KeepAlive(cfg::KeepAlive { id }).encode())
                .unwrap();
        })
    }
}

#[cfg(feature = "test-payload-probe")]
impl Session {
    /// Complete a protocol round trip for deterministic hold race tests.
    #[doc(hidden)]
    pub fn packet_hold_rollover(&self) {
        use sunrise_protocol::payload_negotiation::Phase;
        let writer = self.writer.lock().unwrap();
        let writer = writer.as_ref().unwrap();
        writer
            .transition_protocol(Phase::Configuration, &[3])
            .unwrap();
        writer.transition_protocol(Phase::Play, &[3]).unwrap();
    }
}
