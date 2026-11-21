//! One bounded writer owner. Receiving callbacks never block their own receive loop
//! while a large reply waits for the peer to drain its socket or memory pipe.
use mod_api::{PayloadFrameStage, PayloadId, PayloadSendError, SendPacketError};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use sunrise_mod_runtime::byte_channel::{self, Permit, ReserveError, Sender, TrySendError};
use sunrise_protocol::payload_negotiation::Phase;
use sunrise_protocol::{
    Error, Result,
    connection::{Closer, ConnWriter, Framing},
    frame::FrameSize,
};

use std::thread::JoinHandle;

const CAPACITY: usize = 16 * 1024 * 1024;
// Covers the pending transaction journal record as well as the queue's own node.
const PUBLICATION_OVERHEAD: usize = 512;
enum WriteJob {
    Packet(Vec<u8>),
    Contents(Vec<u8>),
    Disconnect(Option<Vec<u8>>),
    /// A `Droppable` send is waiting: the thread frames what it has room for.
    Droppable,
}

#[derive(Clone)]
pub(crate) struct QueuedWriter(Arc<Inner>);
pub(crate) struct Inner {
    #[cfg(any(test, feature = "test-payload-probe"))]
    // attempts, successful send returns, active input bytes, active calls.
    // ConnWriter::send_contents returns after write_all; it does not flush.
    probe: Arc<[std::sync::atomic::AtomicUsize; 4]>,
    sender: Mutex<Option<Sender<WriteJob>>>,
    droppable: Arc<std::sync::OnceLock<Arc<crate::raw_payloads::droppable::Queue>>>,
    /// A wake-up is already in the channel.
    droppable_woken: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<ConnWriter>>>,
    framing: Framing,
    closer: Arc<Closer>,
    alive: Arc<AtomicBool>,
    retired: AtomicBool,
    terminal: AtomicBool,
    phase: Mutex<(u64, Phase)>,
}
impl std::ops::Deref for QueuedWriter {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

/// Prepared once, charged immediately, invisible to the writer until root commit.
pub struct PreparedPayload {
    permit: Permit<WriteJob>,
    job: WriteJob,
    alive: Arc<AtomicBool>,
}
impl PreparedPayload {
    pub fn publish(self) -> std::result::Result<(), PayloadSendError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(PayloadSendError::Disconnected);
        }
        self.permit
            .publish(self.job)
            .map_err(|_| PayloadSendError::Disconnected)
    }
}
enum PrepareFailure {
    Closed,
    Full,
    Encode(Error),
}
/// An exact physical writer and protocol incarnation, never a Play/CFG boolean.
pub struct PreparedPacket {
    writer: Weak<Inner>,
    incarnation: (u64, u64),
    payload: PreparedPayload,
}
impl PreparedPacket {
    /// Physical session serial and play generation this reservation belongs to.
    pub fn incarnation(&self) -> (u64, u64) {
        self.incarnation
    }

    /// Publish using the original credit; never append behind a protocol ACK or terminal send.
    pub fn try_publish(self) -> std::result::Result<(), SendPacketError> {
        let writer = self.writer.upgrade().ok_or(SendPacketError::NotConnected)?;
        let phase = writer.phase.lock().unwrap();
        if phase.0 != self.incarnation.1 || phase.1 != Phase::Play {
            return Err(SendPacketError::NotInPlay);
        }
        let sender = writer.sender.lock().unwrap();
        if sender.is_none() {
            return Err(SendPacketError::NotConnected);
        }
        self.payload
            .publish()
            .map_err(|_| SendPacketError::NotConnected)
    }
}
impl QueuedWriter {
    pub(crate) fn play_incarnation(&self) -> Option<u64> {
        let phase = self.phase.lock().unwrap();
        (phase.1 == Phase::Play
            && self.alive.load(Ordering::Acquire)
            && !self.is_terminal()
            && !self.retired.load(Ordering::Acquire))
        .then_some(phase.0)
    }

    #[cfg(any(test, feature = "test-payload-probe"))]
    pub(crate) fn diagnostics(&self) -> String {
        let stats = match self.sender.try_lock() {
            Ok(sender) => sender.as_ref().map(|s| s.stats()),
            Err(_) => return "writer sender busy/poisoned".into(),
        };
        let window = stats.map(|s| CAPACITY.saturating_sub(s.reserved_bytes));
        format!(
            "writer window={window:?} capacity={CAPACITY} alive={} terminal={} retired={} stats={stats:?} send_attempts={} send_ok={} active_send_input_bytes={} active_sends={} flush_count=unavailable(no flush call)",
            self.alive.load(Ordering::Acquire),
            self.terminal.load(Ordering::Acquire),
            self.retired.load(Ordering::Acquire),
            self.probe[0].load(Ordering::Relaxed),
            self.probe[1].load(Ordering::Relaxed),
            self.probe[2].load(Ordering::Relaxed),
            self.probe[3].load(Ordering::Relaxed)
        )
    }
    /// Queue the ACK and change incarnation under the generic-publication gate.
    /// Raw publication uses its separate epoch gate; this never acquires it.
    pub(crate) fn transition_protocol(&self, protocol: Phase, ack: &[u8]) -> Result<()> {
        let mut phase = self.phase.lock().unwrap();
        let generation = phase
            .0
            .checked_add(1)
            .ok_or_else(|| Error::new("network phase incarnation exhausted"))?;
        // The actor can still own old-play packets, including a root transaction
        // not yet committed. Invalidate their credit before admitting the ACK.
        if let Some(sender) = self.sender.lock().unwrap().as_ref() {
            sender.revoke_reservations();
        }
        self.send(ack)?;
        *phase = (generation, protocol);
        Ok(())
    }
    pub(crate) fn prepare_packet(
        &self,
        body: Vec<u8>,
        session_serial: u64,
        expected_generation: Option<u64>,
    ) -> std::result::Result<PreparedPacket, SendPacketError> {
        let phase = self.phase.lock().unwrap();
        let (generation, protocol) = *phase;
        if protocol != Phase::Play
            || expected_generation.is_some_and(|expected| expected != generation)
        {
            return Err(SendPacketError::NotInPlay);
        }
        let payload = self
            .prepare_contents(body, true)
            .map_err(|error| match error {
                PrepareFailure::Closed => SendPacketError::NotConnected,
                PrepareFailure::Full => SendPacketError::Backpressure,
                PrepareFailure::Encode(error) => match error.frame_limit() {
                    Some(detail) => SendPacketError::FrameTooLarge {
                        size: detail.size as u64,
                        limit: detail.limit as u64,
                    },
                    None => SendPacketError::EncodeFailed {
                        message: error.to_string(),
                    },
                },
            })?;
        Ok(PreparedPacket {
            writer: Arc::downgrade(&self.0),
            incarnation: (session_serial, generation),
            payload,
        })
    }
    pub(crate) fn new(mut writer: ConnWriter) -> Result<Self> {
        let framing = writer.framing();
        let closer = Arc::new(writer.closer()?);
        let alive = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = byte_channel::channel(CAPACITY).expect("nonzero writer capacity");
        #[cfg(any(test, feature = "test-payload-probe"))]
        let probe = Arc::new(std::array::from_fn::<_, 4, _>(|_| {
            std::sync::atomic::AtomicUsize::new(0)
        }));
        #[cfg(any(test, feature = "test-payload-probe"))]
        let writing = probe.clone();
        let running = alive.clone();
        let droppable: Arc<std::sync::OnceLock<Arc<crate::raw_payloads::droppable::Queue>>> =
            Arc::default();
        let droppable_woken = Arc::new(AtomicBool::new(false));
        let (serving, woken) = (droppable.clone(), droppable_woken.clone());
        let worker = std::thread::Builder::new()
            .name("sunrise-net-write".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    let disconnect = matches!(&job, WriteJob::Disconnect(_));
                    #[cfg(any(test, feature = "test-payload-probe"))]
                    {
                        let bytes = match &job {
                            WriteJob::Packet(b)
                            | WriteJob::Contents(b)
                            | WriteJob::Disconnect(Some(b)) => b.len(),
                            WriteJob::Disconnect(None) | WriteJob::Droppable => 0,
                        };
                        writing[0].fetch_add(1, Ordering::Relaxed);
                        writing[2].store(bytes, Ordering::Relaxed);
                        writing[3].store(1, Ordering::Relaxed);
                    }
                    let mut result = match job {
                        WriteJob::Packet(body) | WriteJob::Disconnect(Some(body)) => {
                            writer.send(&body)
                        }
                        WriteJob::Contents(contents) => writer.send_contents(&contents),
                        WriteJob::Disconnect(None) => Ok(()),
                        WriteJob::Droppable => {
                            woken.store(false, Ordering::Release);
                            Ok(())
                        }
                    };
                    // The writer is free: frame the waiting `Droppable` sends before the next job.
                    if result.is_ok()
                        && !disconnect
                        && let Some(queue) = serving.get()
                    {
                        while let Some(body) = queue.pop(std::time::Instant::now()) {
                            result = writer.send(&body);
                            if result.is_err() {
                                break;
                            }
                        }
                    }
                    #[cfg(any(test, feature = "test-payload-probe"))]
                    {
                        if result.is_ok() {
                            writing[1].fetch_add(1, Ordering::Relaxed);
                        }
                        writing[2].store(0, Ordering::Relaxed);
                        writing[3].store(0, Ordering::Relaxed);
                    }
                    if result.is_err() || disconnect {
                        running.store(false, Ordering::Release);
                        writer.shutdown();
                        break;
                    }
                }
                writer
            })
            .map_err(|error| Error::new(format!("cannot start client writer: {error}")))?;
        Ok(Self(Arc::new(Inner {
            #[cfg(any(test, feature = "test-payload-probe"))]
            probe,
            sender: Mutex::new(Some(sender)),
            droppable,
            droppable_woken,
            worker: Mutex::new(Some(worker)),
            retired: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
            framing,
            closer,
            alive,
            phase: Mutex::new((0, Phase::Configuration)),
        })))
    }
    /// Stop acceptance, cancel unpublished reservations and drain published writes.
    pub(crate) fn drain_and_take(&self) -> Result<ConnWriter> {
        let mut sender = self.sender.lock().unwrap();
        self.retired.store(true, Ordering::Release);
        sender.take();
        drop(sender);
        let worker = self
            .worker
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| Error::new("writer already taken"))?;
        let writer = worker.join().map_err(|_| Error::new("writer panicked"))?;
        if !self.alive.swap(false, Ordering::AcqRel) {
            return Err(Error::new("writer disconnected"));
        }
        Ok(writer)
    }
    /// Serve `queue`'s `Droppable` sends from this writer's thread.
    pub(crate) fn attach_droppable(&self, queue: Arc<crate::raw_payloads::droppable::Queue>) {
        let inner = Arc::downgrade(&self.0);
        queue.set_wake(Box::new(move || {
            let Some(inner) = inner.upgrade() else { return };
            if inner.droppable_woken.swap(true, Ordering::AcqRel) {
                return;
            }
            let sender = inner.sender.lock().unwrap();
            let sent = sender
                .as_ref()
                .is_some_and(|s| s.try_send(WriteJob::Droppable, 0).is_ok());
            if !sent {
                inner.droppable_woken.store(false, Ordering::Release);
            }
        }));
        let _ = self.droppable.set(queue);
    }
    pub(crate) fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::Acquire)
    }
    pub(crate) fn framing(&self) -> Framing {
        self.framing
    }
    pub(crate) fn payload_capacity(&self) -> usize {
        CAPACITY
    }
    pub(crate) fn shutdown(&self) {
        if self.retired.load(Ordering::Acquire) {
            return;
        }
        self.alive.store(false, Ordering::Release);
        if !self.retired.load(Ordering::Acquire) {
            self.closer.close();
        }
        self.sender.lock().unwrap().take();
    }
    /// Direct protocol replies bypass unpublished play credit, but still join the
    /// same FIFO and observe backpressure from the writer's actual backlog.
    pub(crate) fn send(&self, body: &[u8]) -> Result<()> {
        self.enqueue(body.to_vec(), true).map_err(|error| {
            Error::new(if error == SendPacketError::Backpressure {
                "client writer backpressure"
            } else {
                "client writer disconnected"
            })
        })
    }
    pub(crate) fn send_engine_play(
        &self,
        body: Vec<u8>,
    ) -> std::result::Result<(), SendPacketError> {
        let result = self.enqueue(body, false);
        if result.is_err() && !self.is_terminal() {
            self.shutdown();
        }
        result
    }
    fn enqueue(
        &self,
        body: Vec<u8>,
        bypass_revocable: bool,
    ) -> std::result::Result<(), SendPacketError> {
        let capacity = body.capacity();
        let sender = self.sender.lock().unwrap();
        if !self.alive.load(Ordering::Acquire) {
            return Err(SendPacketError::NotConnected);
        }
        let sender = sender.as_ref().ok_or(SendPacketError::NotConnected)?;
        let job = WriteJob::Packet(body);
        let result = if bypass_revocable {
            sender.try_send_bypassing_revocable(job, capacity)
        } else {
            sender.try_send(job, capacity)
        };
        match result {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.alive.store(false, Ordering::Release);
                self.closer.close();
                Err(SendPacketError::Backpressure)
            }
            Err(TrySendError::Disconnected(_)) => Err(SendPacketError::NotConnected),
        }
    }
    /// Drain preceding writes and close. A locally refused frame has no body to send.
    pub(crate) fn disconnect(&self, body: Option<Vec<u8>>) {
        let mut sender = self.sender.lock().unwrap();
        self.terminal.store(true, Ordering::Release);
        let Some(queue) = sender.take() else {
            return;
        };
        queue.revoke_reservations();
        let bytes = body.as_ref().map_or(0, Vec::capacity);
        if queue.try_send(WriteJob::Disconnect(body), bytes).is_err() {
            self.closer.close();
        }
    }
    pub(crate) fn prepare_payload(
        &self,
        body: Vec<u8>,
        id: &PayloadId,
    ) -> std::result::Result<PreparedPayload, PayloadSendError> {
        self.prepare_contents(body, false)
            .map_err(|failure| match failure {
                PrepareFailure::Closed => PayloadSendError::Disconnected,
                PrepareFailure::Full => PayloadSendError::Backpressure,
                PrepareFailure::Encode(error) => {
                    if let Some(detail) = error.frame_limit() {
                        PayloadSendError::FrameTooLarge {
                            id: id.clone(),
                            stage: match detail.stage {
                                FrameSize::Encoded => PayloadFrameStage::Encoded,
                                FrameSize::Uncompressed => PayloadFrameStage::Uncompressed,
                            },
                            size: detail.size as u64,
                            limit: detail.limit as u64,
                        }
                    } else {
                        PayloadSendError::EncodeFailed(error.to_string())
                    }
                }
            })
    }
    fn prepare_contents(
        &self,
        body: Vec<u8>,
        revocable: bool,
    ) -> std::result::Result<PreparedPayload, PrepareFailure> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(PrepareFailure::Closed);
        }
        let contents = self
            .framing
            .contents_owned(body)
            .map_err(PrepareFailure::Encode)?;
        let capacity = contents.capacity().saturating_add(PUBLICATION_OVERHEAD);
        let sender = self.sender.lock().unwrap();
        if !self.alive.load(Ordering::Acquire) {
            return Err(PrepareFailure::Closed);
        }
        let sender = sender.as_ref().ok_or(PrepareFailure::Closed)?;
        let permit = if revocable {
            sender.reserve_revocable(capacity)
        } else {
            sender.reserve(capacity)
        }
        .map_err(|error| match error {
            ReserveError::Full => PrepareFailure::Full,
            ReserveError::Disconnected => PrepareFailure::Closed,
        })?;
        Ok(PreparedPayload {
            permit,
            job: WriteJob::Contents(contents),
            alive: self.alive.clone(),
        })
    }
}
#[cfg(test)]
mod tests;

impl Drop for Inner {
    fn drop(&mut self) {
        if !self.retired.load(Ordering::Acquire) && !self.terminal.load(Ordering::Acquire) {
            self.closer.close();
        }
    }
}
