# Audio streams

Play audio your native mod makes: a music player, a radio, a text reader or proximity voice chat. Give the mixer a source it can read on demand, or push interleaved signed 16-bit PCM from an async producer. See the [API reference](../../reference/api.md) for the generated signatures.

Streams use a pool separate from vanilla's streaming slots. They are not sound instances: nothing in `sounds.json` names them, and no sound hooks fire for them.

To change the client's situational music instead of supplying PCM, use the `music` hook with
a registered sound event in `Music::new`, or a vanilla definition such as `Music::menu()`
or `Music::game()`. Use `music_layers` to layer sound events over the selected music.
For a music picker in your mod, choose the definitions and order in your screen and give
the entries your own translation keys.

## Pull PCM on demand

Implement `AudioStream` when your mod can generate or decode PCM as the mixer needs it. For example, this source plays an owned clip of mono samples at 48 kHz:

```rust
use mod_api::{AudioRead, AudioRefusal, AudioStream, AudioStreamRef, ClientHost, SoundSource, StreamSpec};

struct Clip(std::vec::IntoIter<i16>);

impl AudioStream for Clip {
    fn read(&mut self, pcm: &mut [i16]) -> AudioRead {
        let mut frames = 0;
        for (out, sample) in pcm.iter_mut().zip(self.0.by_ref()) {
            *out = sample;
            frames += 1; // Mono: one sample per frame.
        }
        if frames == 0 {
            AudioRead::Eof
        } else {
            AudioRead::Frames(frames)
        }
    }
}

fn play_clip(host: &dyn ClientHost, mono_pcm: Vec<i16>) -> Result<AudioStreamRef, AudioRefusal> {
    let spec = StreamSpec::new(SoundSource::Music, 1, 48_000);
    host.open_audio_stream(&spec, Box::new(Clip(mono_pcm.into_iter())))
}
```

The mixer owns the `Send + 'static` source. Keep `read` bounded and nonblocking: all pull streams on the output share a worker, so one slow source delays the others. Do not wait on I/O, locks, another thread or a tick. A source has no `Host` or world borrow and must not call back into the game. Use the preallocated buffer it lends you.

Write up to `pcm.len() / channels` whole interleaved frames into the cleared buffer and return `AudioRead::Frames(n)` with the **frame** count, not the sample count. A short read plays what was supplied; `Frames(0)` is temporary silence. The worker tries again on later demand. A count larger than the buffer can hold stops that stream and is logged. When no more PCM will arrive, return `AudioRead::Eof` on a read after reporting the final frames. The mixer drains the queued frames once the worker drops the source successfully with the owner still enabled. If destruction fails, the stream stops without draining. The worker drops an admitted source off the device callback, even if no further callback runs after you stop it. `push` on a pull handle always returns zero.

If a pull source panics while the worker reads or drops it, the client logs the owning mod and shows an error toast, even if a mod cancels the HUD. The player's hide-GUI setting still applies. The panic wakes the worker to retire that mod's live pull sources without waiting for another device callback. This also applies if the worker rejects admission and the source panics while being dropped on the opening thread. The mod cannot open another pull stream until the client restarts. Other mods' streams continue on the same worker; push streams are unaffected. Disabling a mod also retires its live pull sources. The worker discards any read results still in flight and stops the affected streams without draining. A source returning `AudioRead::Eof` is still live until it has been dropped successfully with the owner still enabled; only streams that already completed normal EOF retirement keep draining queued PCM.

## Push PCM from an async producer

Use `open_push_audio_stream` for audio that arrives independently of the mixer, for example in a `Network`-thread payload handler. This opens a mono voice stream that follows the speaker:

```rust
use mod_api::{AudioRefusal, AudioStreamRef, ClientHost, EntityRef, SoundSource, StreamPlacement, StreamSpec};

fn open_voice(host: &dyn ClientHost, speaker: EntityRef) -> Result<AudioStreamRef, AudioRefusal> {
    let spec = StreamSpec::new(SoundSource::Voice, 1, 48_000)
        .with_placement(StreamPlacement::Entity(speaker))
        .with_range(32.0);
    host.open_push_audio_stream(&spec)
}

// On the producer thread, with decoded mono PCM. Drop the suffix or retry it later.
fn push_voice<'a>(stream: &AudioStreamRef, mono_pcm: &'a [i16]) -> &'a [i16] {
    let accepted_frames = stream.push(mono_pcm) as usize;
    &mono_pcm[accepted_frames..]
}
```

`push` writes into the ring allocated at open, without allocating or waiting for the mixer or a tick. The ring holds one second of source audio (`AUDIO_STREAM_RING_MAX_MS`). Playback starts or resumes with the first available frame; a dry ring plays silence without stopping. If the ring fills, `push` accepts only the prefix that fits. Multiply the returned frame count by the channel count to find where the refused samples start. An overlapping push to the same stream accepts nothing, so use one producer per stream. If your network audio needs a jitter buffer, keep it on the producer side.

## Format and controls

Both openers are client host calls available to the calling mod from client setup onward. They refuse with `AudioRefusal`, without opening anything, if the client has no sound output, the call is unavailable in this context, the spec is invalid, or the limit of 32 open streams per mod or 64 across all mods is reached. A pull open can also return `AudioRefusal::WorkerUnavailable` if the worker cannot start or take the source, or `AudioRefusal::PullDisabled` if the calling mod's pull callbacks are disabled after a panic.

| `StreamSpec` field | Meaning |
|---|---|
| `category` | The mixer group, scaled by master times the group like every sound. Voice chat uses `SoundSource::Voice`, the “Voice/Speech” slider. |
| `channels` | 1 or 2. Only mono is positioned. |
| `rate` | 8000 to 48000 Hz. The mixer resamples to the output's rate. |
| `placement` | `Flat`, `At([x, y, z])` or `Entity(entity)`, in the client's current level. |
| `range` | Blocks at which a positioned stream has faded to silence, in a straight line (`AL_LINEAR_DISTANCE`, as a sound's range). |
| `volume` | 0 to 1, before the group's and the master's. |

Use the `AudioStreamRef` handle to set placement or volume, read state, or stop playback. **Dropping the handle stops the stream**, so keep it for as long as you want playback. A stopped stream frees its place in the pool even if you keep its handle, and `push` returns zero. Muffling (`muffle_sounds`) applies to both kinds of streams.

- `Playing`: open and placed (or flat), even if muted or dry.
- `Unplaced`: open, but at a place nobody hears (see below).
- `Stopped`: stopped explicitly, dropped, or lost sound output. A pull stream also stops after normal EOF retirement and drain, an invalid frame count, a source panic, failed source destruction, or when the worker retires its live source because its owning mod or pull callbacks were disabled. A stopped stream never plays again; if output is lost, open a new stream once it is back. Changing devices successfully does not stop streams.

## The player's switch

With Sunrise Extras' “Mod audio streams” switch off, streams still open and advance in silence. A mod needs no special case, and switching it back on does not play a backlog. The Extras switch is in `crates/sunrise-official/src/mod_audio.rs`; the engine only owns the gain it sets.

## Placement

- A **mono** stream may be `Flat` (at the listener, no panning and no fading), at a point or at an entity. It is panned as a positioned sound is and fades linearly to silence at `range`.
- A **stereo** stream is `Flat` only; a stereo spec with another placement is refused at open with `SpecFault::StereoPositioned`, and `set_placement` returns false.
- An `Entity` stream follows its entity every frame. While the entity is untracked, unloaded or removed the stream is silent and reads `Unplaced`, but its audio is still consumed, so no latency builds up. It plays again if the same entity is tracked again.
- When the listener changes dimension (or leaves the level) every positional stream goes `Unplaced` until the mod calls `set_placement` again, since a point or entity of another level is never heard.

## Network audio

`StreamPlacement`, `StreamSpec`, `AudioStreamState`, `AudioRefusal` and `SpecFault` are local values,
not [Wire](../../concepts/wire-types.md) types. For network audio, send a separate
[typed payload](networking.md#typed-payloads) with the data the receiver needs, such as the emitter's
server-assigned network entity ID and the encoded audio:

```rust
#[derive(mod_api::Wire)]
struct VoiceFrame {
    emitter_id: u32,
    encoded_audio: Vec<u8>,
}
```

Put the emitter's `entity.id()` in the clientbound frame, not its `EntityRef`: the handle belongs to
its own world epoch. On the receiving client's main thread, use `host.entity(frame.emitter_id)` to
resolve a handle in the current level. If it returns `None`, skip the frame. Otherwise, use
`StreamPlacement::Entity(entity)` for the local stream. Scope the IDs and any cached handles to the
current level and connection; do not carry pending frames into a new level. A network-thread handler
can feed a stream already opened and placed on the main thread.

## Threading

- **Client host:** Open a stream where the host runs. The client tracks placements and reaps stopped streams; it does not have to refill PCM on a tick.
- **Pull worker:** Calls each source's `read` serially without holding mixer or pool locks. The device callback requests PCM for the next buffer; if several requests arrive before the worker reads, the latest wins.
- **Async producer:** Decode or generate PCM on your producer thread and `push` it when ready, independently of the client tick. Keep one producer per stream. The `AudioStreamRef` is `Send`: `push`, `set_placement`, `set_volume`, `state` and `stop` work from any thread without taking the mixer's lock.
- **Device callback:** Drains the queued PCM and mixes it. It does not call your source or wait for a refill. If the producer falls behind, the missing audio is silent.

## Microphone capture

List available input devices with `host.microphone_devices()` and open live PCM capture on a `ClientHost` from client setup onward with `host.open_microphone(&spec)`:

```rust
use mod_api::{ClientHost, MicDevice, MicRef, MicRefusal, MicSpec};

fn open_mic(host: &dyn ClientHost, preferred: Option<&MicDevice>) -> Result<MicRef, MicRefusal> {
    let mut spec = MicSpec::new(1, 48_000); // 1 or 2 channels, 8000 to 48000 Hz.
    if let Some(device) = preferred {
        spec = spec.with_device(&device.id);
    }
    host.open_microphone(&spec)
}

// On any thread: read whole interleaved signed 16-bit PCM frames without waiting on a tick.
fn read_mono<'a>(mic: &MicRef, pcm: &'a mut [i16]) -> &'a [i16] {
    let frames = mic.read(pcm) as usize;
    &pcm[..frames]
}
```

Each entry returned by `host.microphone_devices()` is a `MicDevice { id, label }`. Show `device.label` in UI and pass `device.id` to `MicSpec::with_device` to target that endpoint. If it vanishes, opening its ID returns `MicRefusal::NoDevice`; refresh the list rather than substituting a device with the same label. Treat the ID as opaque: pass it through unchanged. To intentionally select by display label, use `MicSpec::with_label`. A label shared by multiple available inputs returns `MicRefusal::AmbiguousDevice`. If discovery fails before it can establish a unique match, opening by label returns that discovery error instead of choosing the first input it found. Use an ID to target a specific input. An inaccessible endpoint does not hide other available inputs when listing.

`MicRef` is `Send + Sync` and safe to read from any thread off the OS audio callback, with no tick or codec required. `mic.read(&mut pcm)` is nonblocking: it writes whole interleaved signed 16-bit PCM frames into the returned prefix `pcm[..frames * channels]`, leaves any remaining suffix untouched, and returns `0` without modifying `pcm` if no whole frames are ready or another reader is concurrently draining the same handle. Capture feeds a bounded 500 ms ring (`MIC_RING_MAX_MS`) that drops the oldest frames if the reader falls behind; `mic.dropped_frames()` reports the cumulative dropped frame count.

Call `mic.close()` or drop `MicRef` to stop capture and release the OS input stream synchronously before returning. `mic.is_open()` returns `false` after close or if the input device disconnects or errors (in which case the backend stream is released promptly without waiting for a later call). Handle `MicRefusal` directly:

- `OsDenied`: the operating system denied capture, blocked lookup of the selected input (for example, ALSA denied access to a card with a previously listed ID or label), or prevented listing any available inputs. On macOS the system can ask for microphone access before probing an input on the first open. Listing and opening a label that matches only outputs do not request access.
- `Busy`: the input device is busy or held in exclusive use.
- `NoDevice`: no default input device exists, the requested ID or label matched no input, or client audio is disabled.
- `AmbiguousDevice`: a device label matched more than one input device (pass `MicDevice::id` instead).
- `BadSpec(MicSpecFault)`: `channels` is not 1 or 2, or `rate` is outside 8000 to 48000 Hz (`MIC_RATE_MIN`..=`MIC_RATE_MAX`).
- `OnServer` / `Unavailable`: called on a server or outside client setup or later.
- `DeviceFailed`: input discovery failed, or the input device or capture worker failed to open or start.

`MicDevice`, `MicRef`, `MicSpec`, `MicTarget`, `MicSpecFault` and `MicRefusal` are client-local, not [Wire](../../concepts/wire-types.md) types. The engine never records or sends microphone audio; mods own their own echo cancellation, metering, encoding and networking.

## Known limits

- There is no echo cancellation: a mod that plays voice while capturing the microphone must deal with its own echo.
- Voice is unmoderated: the engine neither records, filters nor reports a stream's audio. Replay recordings capture voice payloads like any other payload.
- These APIs play and capture raw PCM. To send time-sensitive audio between players, use [droppable play payloads](networking.md#droppable-delivery-audio-live-data). Your mod brings its own codec as an ordinary dependency and owns its buffering and gap handling.
