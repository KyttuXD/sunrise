//! Sound-event groups for the Sunrise Music settings page.

use super::{MusicGroup, MusicTrack};
use crate::sound_events::SoundRegistry;
use mod_api::client::music::Music;
use std::collections::BTreeMap;

/// The Music page's groups: each built-in situation's own tracks first, then
/// every other `music.` event's tracks that no earlier group listed, under
/// its dimension's biomes. A track several pools share is listed once, from
/// the first pool that names it, so "play now" always has a pool that plays
/// it and no track is left without a row.
pub fn track_groups(registry: &SoundRegistry) -> Vec<MusicGroup> {
    let mut groups: Vec<MusicGroup> = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    // The picker owns both the headings and their order.
    let builtin = [
        ("sunrise.pages.music.menu", Music::menu()),
        ("sunrise.pages.music.overworld", Music::game()),
        ("sunrise.pages.music.creative", Music::creative()),
        ("sunrise.pages.music.underwater", Music::under_water()),
        ("sunrise.pages.music.end", Music::end()),
        ("sunrise.pages.music.boss", Music::end_boss()),
        ("sunrise.pages.music.credits", Music::credits()),
    ];
    for (label, music) in &builtin {
        let tracks = registry.tracks(&music.sound);
        if tracks.is_empty() {
            continue;
        }
        listed.extend(tracks.iter().cloned());
        groups.push(MusicGroup {
            label: (*label).to_owned(),
            tracks: tracks
                .into_iter()
                .map(|id| MusicTrack {
                    event: music.sound.clone(),
                    id,
                })
                .collect(),
        });
    }
    // `music_disc.*` (records) do not start with "music.".
    let mut events: Vec<&String> = registry
        .names()
        .filter(|event| event.starts_with("minecraft:music."))
        .collect();
    events.sort();
    // Keyed by the heading's rank, so the dimensions keep their order however
    // the event ids sort.
    let mut rest: BTreeMap<u8, (&'static str, Vec<MusicTrack>)> = BTreeMap::new();
    for event in events {
        if builtin.iter().any(|(_, music)| music.sound == *event) {
            continue;
        }
        let tracks: Vec<MusicTrack> = registry
            .tracks(event)
            .into_iter()
            .filter(|id| !listed.contains(id))
            .map(|id| MusicTrack {
                event: event.clone(),
                id,
            })
            .collect();
        if tracks.is_empty() {
            continue;
        }
        listed.extend(tracks.iter().map(|track| track.id.clone()));
        let (rank, label) = biome_group(event);
        rest.entry(rank)
            .or_insert_with(|| (label, Vec::new()))
            .1
            .extend(tracks);
    }
    groups.extend(rest.into_values().map(|(label, tracks)| MusicGroup {
        label: label.to_owned(),
        tracks,
    }));
    groups
}
/// The heading a biome-only pool's tracks go under: vanilla plays these
/// tracks in several biomes, so no heading names a single one.
fn biome_group(event: &str) -> (u8, &'static str) {
    let path = event.strip_prefix("minecraft:").unwrap_or(event);
    if path.starts_with("music.overworld.") {
        (1, "sunrise.pages.music.overworld_biomes")
    } else if path.starts_with("music.nether.") {
        (2, "sunrise.pages.music.nether_biomes")
    } else if path.starts_with("music.end.") {
        (3, "sunrise.pages.music.end_biomes")
    } else {
        (4, "sunrise.pages.music.other")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_music_page_lists_every_track_once_from_a_pool_that_plays_it() {
        let mut registry = SoundRegistry::default();
        registry.add_file(
            "minecraft",
            br#"{
                "music.game": {"sounds": ["music/game/shared", "music/game/second"]},
                "music.menu": {"sounds": ["music/menu/one"]},
                "music.overworld.forest": {"sounds": ["music/game/shared", "music/game/bromeliad"]},
                "music.overworld.swamp": {"sounds": ["music/game/bromeliad", "music/game/aerie"]},
                "music.nether.warped_forest": {"sounds": ["music/game/rubedo"]},
                "music_disc.cat": {"sounds": ["records/cat"]},
                "note": {"sounds": ["block/note/harp"]}
            }"#,
            &|_, _| true,
        );
        let groups = track_groups(&registry);
        let summary: Vec<(&str, Vec<&str>)> = groups
            .iter()
            .map(|group| {
                (
                    group.label.as_str(),
                    group
                        .tracks
                        .iter()
                        .map(|track| track.id.rsplit('/').next().unwrap())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("sunrise.pages.music.menu", vec!["one"]),
                ("sunrise.pages.music.overworld", vec!["shared", "second"]),
                // A biome pool's own track, from the pool that plays it.
                (
                    "sunrise.pages.music.overworld_biomes",
                    vec!["bromeliad", "aerie"]
                ),
                ("sunrise.pages.music.nether_biomes", vec!["rubedo"]),
            ]
        );
        // Each row plays from a pool that has the track; no track repeats.
        let mut seen = Vec::new();
        for group in &groups {
            for track in &group.tracks {
                assert!(!seen.contains(&track.id), "{} listed twice", track.id);
                seen.push(track.id.clone());
                assert!(
                    registry.tracks(&track.event).contains(&track.id),
                    "{} does not play {}",
                    track.event,
                    track.id
                );
            }
        }
        // Records and other events are not music.
        assert!(seen.iter().all(|id| !id.contains("records/")));
    }
}
