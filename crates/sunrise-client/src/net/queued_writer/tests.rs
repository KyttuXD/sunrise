use super::*;
use std::time::Duration;
use sunrise_protocol::connection::{Connection, Pipe};

#[test]
fn memory_payload_is_invisible_until_commit_and_keeps_large_body_exact() {
    let (a, b) = Pipe::pair();
    let (_, writer) = Connection::local(a).split();
    let writer = QueuedWriter::new(writer).unwrap();
    let mut peer = Connection::local(b);
    peer.reader
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let id = PayloadId("test:raw".into());
    let length = sunrise_protocol::frame::MAX_UNCOMPRESSED + 1;
    let mut body = vec![42; length];
    body[0] = 7;
    let pending = writer.prepare_payload(body, &id).unwrap();
    assert!(peer.recv().is_err(), "uncommitted packet reached the pipe");
    assert!(
        writer
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stats()
            .reserved_bytes
            >= length
    );
    assert_eq!(
        writer
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stats()
            .queued_items,
        0
    );
    peer.reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    pending.publish().unwrap();
    let (packet, bytes) = peer.recv().unwrap();
    assert_eq!(packet, 7);
    assert_eq!(bytes.len(), length - 1);
    assert!(bytes.iter().all(|byte| *byte == 42));
}

#[test]
fn abort_refunds_journal_and_buffer_space_and_shutdown_prevents_publication() {
    let (a, _b) = Pipe::pair();
    let (_, writer) = Connection::local(a).split();
    let writer = QueuedWriter::new(writer).unwrap();
    let id = PayloadId("test:raw".into());
    let pending = writer
        .prepare_payload(Vec::with_capacity(10000), &id)
        .unwrap();
    assert!(
        writer
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stats()
            .reserved_bytes
            >= 10000 + PUBLICATION_OVERHEAD
    );
    drop(pending);
    assert_eq!(
        writer
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stats()
            .reserved_bytes,
        0
    );
    let pending = writer.prepare_payload(vec![7], &id).unwrap();
    writer.shutdown();
    assert_eq!(pending.publish(), Err(PayloadSendError::Disconnected));
    assert!(writer.sender.lock().unwrap().is_none());
}

#[test]
fn handoff_drains_order_cancels_permits_and_stale_clones_do_not_close() {
    let (a, b) = Pipe::pair();
    let (_, physical) = Connection::local(a).split();
    let queue = QueuedWriter::new(physical).unwrap();
    let stale = queue.clone();
    let mut peer = Connection::local(b);
    peer.reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let unpublished = queue
        .prepare_payload(vec![9, 99], &PayloadId("test:pending".into()))
        .unwrap();
    queue.send(&[1, 11]).unwrap();
    queue
        .prepare_payload(vec![2, 22], &PayloadId("test:ready".into()))
        .unwrap()
        .publish()
        .unwrap();
    let mut physical = queue.drain_and_take().unwrap();
    assert_eq!(unpublished.publish(), Err(PayloadSendError::Disconnected));
    assert!(stale.send(&[8]).is_err());
    drop(stale);
    drop(queue);
    physical.send(&[3, 33]).unwrap();
    assert_eq!(peer.recv().unwrap(), (1, vec![11]));
    assert_eq!(peer.recv().unwrap(), (2, vec![22]));
    assert_eq!(peer.recv().unwrap(), (3, vec![33]));
}

#[test]
fn crossed_large_memory_writes_never_hold_the_receive_loop() {
    let (a, b) = Pipe::pair();
    let (mut read_a, write_a) = Connection::local(a).split();
    let (mut read_b, write_b) = Connection::local(b).split();
    read_a
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    read_b
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let a = QueuedWriter::new(write_a).unwrap();
    let b = QueuedWriter::new(write_b).unwrap();
    let length = 9 * 1024 * 1024;
    let mut body = vec![41; length];
    body[0] = 7;
    let id = PayloadId("test:large".into());
    a.prepare_payload(body.clone(), &id)
        .unwrap()
        .publish()
        .unwrap();
    b.prepare_payload(body, &id).unwrap().publish().unwrap();
    for read in [&mut read_a, &mut read_b] {
        let (id, bytes) = read.recv().unwrap();
        assert_eq!(id, 7);
        assert_eq!(bytes.len(), length - 1);
        assert!(bytes.iter().all(|b| *b == 41));
    }
    a.shutdown();
    b.shutdown();
}

#[test]
fn terminal_disconnect_drains_even_when_last_handle_drops() {
    let (a, b) = Pipe::pair();
    let (_, physical) = Connection::local(a).split();
    let queue = QueuedWriter::new(physical).unwrap();
    let mut peer = Connection::local(b);
    peer.reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    queue.transition_protocol(Phase::Play, &[4]).unwrap();
    let pending = queue.prepare_packet(vec![3, 33], 7, None).unwrap();
    queue.send(&[1, 11]).unwrap();
    queue.disconnect(Some(vec![2, 22]));
    assert_eq!(pending.try_publish(), Err(SendPacketError::NotConnected));
    assert!(queue.send(&[3]).is_err());
    drop(queue);
    assert_eq!(peer.recv().unwrap(), (4, vec![]));
    assert_eq!(peer.recv().unwrap(), (1, vec![11]));
    assert_eq!(peer.recv().unwrap(), (2, vec![22]));
    assert!(peer.recv().is_err());
}

#[test]
fn control_and_protocol_ack_pass_a_full_unpublished_play_window() {
    let (a, b) = Pipe::pair();
    let (_, physical) = Connection::local(a).split();
    let queue = QueuedWriter::new(physical).unwrap();
    let mut peer = Connection::local(b);
    peer.reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    queue.transition_protocol(Phase::Play, &[4]).unwrap();
    assert_eq!(peer.recv().unwrap(), (4, vec![]));

    // Spend the entire window on a packet the game actor still owns. No jobs
    // are waiting for the transport and the peer is reading each message.
    let window = CAPACITY - PUBLICATION_OVERHEAD - byte_channel::message_overhead::<WriteJob>();
    let held = queue.prepare_packet(vec![5; window], 7, None).unwrap();
    assert_eq!(
        queue.prepare_packet(vec![5], 7, None).err(),
        Some(SendPacketError::Backpressure)
    );
    queue.send(&[6, 66]).unwrap();
    assert_eq!(peer.recv().unwrap(), (6, vec![66]));

    queue
        .transition_protocol(Phase::Configuration, &[7])
        .unwrap();
    assert_eq!(peer.recv().unwrap(), (7, vec![]));
    queue.transition_protocol(Phase::Play, &[8]).unwrap();
    assert_eq!(peer.recv().unwrap(), (8, vec![]));
    // The old token is still live on the actor; credit was returned by the transition.
    let fresh = queue.prepare_packet(vec![9, 99], 7, None).unwrap();
    assert_eq!(held.try_publish(), Err(SendPacketError::NotInPlay));
    fresh.try_publish().unwrap();
    assert_eq!(peer.recv().unwrap(), (9, vec![99]));
}

/// Every packet the peer reads until a closing sentinel (id 0x7e), as (id, payload).
fn read_until_sentinel(peer: &mut Connection) -> Vec<(i32, Vec<u8>)> {
    let mut all = Vec::new();
    loop {
        let packet = peer.recv().unwrap();
        if packet.0 == 0x7e {
            return all;
        }
        all.push(packet);
    }
}

#[test]
fn the_writer_thread_frames_waiting_droppable_packets() {
    let (a, b) = Pipe::pair();
    let (_, writer) = Connection::local(a).split();
    let writer = QueuedWriter::new(writer).unwrap();
    let queue = crate::raw_payloads::droppable::Queue::new(u32::MAX, None);
    writer.attach_droppable(queue.clone());
    let mut peer = Connection::local(b);
    peer.reader
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    for n in 0..3u8 {
        queue.push(
            std::time::Instant::now(),
            &Arc::default(),
            vec![0x40 + n, n],
        );
    }
    for n in 0..3u8 {
        assert_eq!(peer.recv().unwrap(), (i32::from(0x40 + n), vec![n]));
    }
}

#[test]
fn a_dropped_droppable_leaves_the_framed_stream_as_if_never_sent() {
    let run = |push: bool| {
        let (a, b) = Pipe::pair();
        let (_, writer) = Connection::local(a).split();
        let writer = QueuedWriter::new(writer).unwrap();
        // A zero budget drops every droppable send at the queue.
        let queue = crate::raw_payloads::droppable::Queue::new(0, None);
        writer.attach_droppable(queue.clone());
        let mut peer = Connection::local(b);
        peer.reader
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writer.send(&[3, 1, 2]).unwrap();
        if push {
            queue.push(std::time::Instant::now(), &Arc::default(), vec![0x40, 9]);
        }
        writer.send(&[4, 5]).unwrap();
        writer.send(&[0x7e]).unwrap();
        read_until_sentinel(&mut peer)
    };
    assert_eq!(run(true), run(false));
}
