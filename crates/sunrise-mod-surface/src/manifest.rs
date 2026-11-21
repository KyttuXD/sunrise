//! The published mod manifest's file boundary, shared by the package reader and the standalone fit scanner.

use std::io::{self, Read};

/// The manifest's file name in a `.smod` or a mod folder.
pub const MANIFEST: &str = "sunrise-mod.toml";
/// Maximum bytes in the complete manifest, including properties and dependency explanations.
pub const MAX_MANIFEST: u64 = 32 << 20;

/// Read the complete manifest as UTF-8 without reading beyond its published byte limit plus one.
pub fn read(reader: impl Read) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(MAX_MANIFEST + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("over {} MiB", MAX_MANIFEST >> 20),
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
