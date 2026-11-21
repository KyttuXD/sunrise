use mod_api::SoundPlay;

/// Whether `id` is a fully qualified sound event key, with vanilla `Identifier` syntax.
pub(in crate::game) fn sound_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    let allowed = |c: char, slash: bool| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c) || (slash && c == '/')
    };
    !namespace.is_empty()
        && namespace != ".."
        && !path.is_empty()
        && namespace.chars().all(|c| allowed(c, false))
        && path.chars().all(|c| allowed(c, true))
}

/// Validate the raw event name and sound source of a level write.
pub(in crate::game) fn mod_sound_ok(sound: &str, source: usize) -> bool {
    sound_id(sound) && source < sunrise_client::settings::SOUND_SOURCES.len()
}

/// Whether `play_local_sound` takes this sound: a sound event's id (`sound_id`, as
/// `native_play_sound` takes), a finite position, a volume and pitch at or above 0, a repeat
/// gap that isn't negative, valid effects, and a start a minute away at most.
pub(super) fn local_sound_ok(sound: &SoundPlay, after: u32) -> bool {
    sound_id(&sound.identifier)
        && sound
            .position
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 30_000_000.0)
        && sound.volume.is_finite()
        && sound.volume >= 0.0
        && sound.pitch.is_finite()
        && sound.pitch >= 0.0
        && sound.delay >= 0
        && sound.effects.valid()
        && after <= 1200
}

/// `params` with each value that isn't finite as 0, so a mod's mistake can't
/// put a NaN in every pixel of a pack's picture.
pub(super) fn finite_params(params: [[f32; 4]; 8]) -> [[f32; 4]; 8] {
    params.map(|v| v.map(|x| if x.is_finite() { x } else { 0.0 }))
}

/// `v` within `lo..=hi`; `lo` when it is not a number.
pub(super) fn finite(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { lo }
}
