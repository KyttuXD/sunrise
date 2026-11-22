//! Bounded custom-payload fragments; activate only after the complete snapshot validates.
use super::*;
use sunrise_protocol::packets::custom_payload::CLIENTBOUND_LIMIT;

const MAGIC: &[u8; 4] = b"DRS1";
const HEADER: usize = 12;
// Vanilla's ClientboundCustomPayloadPacket.MAX_PAYLOAD_SIZE includes our fragment header.
pub const CHUNK_DATA: usize = CLIENTBOUND_LIMIT - HEADER;
static ASSEMBLY: Mutex<Assembly> = Mutex::new(Assembly { pending: None });

fn snapshot_too_large() -> String {
    format!(
        "Datapack registry snapshot exceeds the {}-byte transport limit.",
        mod_api::MAX_SNAPSHOT_BYTES
    )
}

pub fn chunks(bytes: &[u8]) -> Result<impl Iterator<Item = Vec<u8>> + '_, String> {
    if bytes.is_empty() {
        return Err("Cannot send an empty datapack registry snapshot.".into());
    }
    if bytes.len() > mod_api::MAX_SNAPSHOT_BYTES {
        return Err(snapshot_too_large());
    }
    let total = bytes.len() as u32;
    Ok(bytes.chunks(CHUNK_DATA).enumerate().map(move |(i, part)| {
        let mut packet = Vec::with_capacity(HEADER + part.len());
        packet.extend_from_slice(MAGIC);
        packet.extend_from_slice(&total.to_le_bytes());
        packet.extend_from_slice(&((i * CHUNK_DATA) as u32).to_le_bytes());
        packet.extend_from_slice(part);
        packet
    }))
}

#[derive(Default)]
struct Assembly {
    pending: Option<(usize, Vec<u8>)>,
}
impl Assembly {
    fn push(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let previous = self.pending.take();
        let bad = || "Invalid or out-of-order datapack registry snapshot fragment.".to_string();
        let (header, part) = packet.split_at_checked(HEADER).ok_or_else(bad)?;
        if !header.starts_with(MAGIC) {
            return Err(bad());
        }
        let total = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        let mut bytes = if let Some((expected, bytes)) = previous {
            if total != expected || offset != bytes.len() {
                return Err(bad());
            }
            bytes
        } else {
            if offset != 0 {
                return Err(bad());
            }
            if total > mod_api::MAX_SNAPSHOT_BYTES {
                return Err(snapshot_too_large());
            }
            Vec::new()
        };
        if offset >= total || part.len() != CHUNK_DATA.min(total - offset) {
            return Err(bad());
        }
        bytes.extend_from_slice(part);
        if bytes.len() == total {
            Ok(Some(bytes))
        } else {
            self.pending = Some((total, bytes));
            Ok(None)
        }
    }
}

pub fn clear_transfer() {
    ASSEMBLY.lock().unwrap_or_else(|e| e.into_inner()).pending = None;
}
pub fn finish_transfer() -> Result<(), String> {
    if ASSEMBLY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pending
        .is_some()
    {
        Err("Incomplete datapack registry snapshot at the end of configuration.".into())
    } else {
        Ok(())
    }
}
pub fn accept_chunk(packet: &[u8]) -> Result<(), String> {
    let complete = ASSEMBLY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(packet)?;
    if let Some(bytes) = complete {
        accept(&bytes)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "datapack_transport_tests.rs"]
mod tests;
