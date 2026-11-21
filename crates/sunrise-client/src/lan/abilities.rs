//! A server player's `Abilities`: the one place the flags and speeds live.
//! `GameType.updatePlayerAbilities` sets them from the game mode when the mode
//! changes; a mod may write them after, until the next change of mode.
use sunrise_protocol::packets::play;

/// `Abilities`, with vanilla's defaults.
#[derive(Clone, Copy, Debug)]
pub struct PlayerAbilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub may_fly: bool,
    pub instabuild: bool,
    pub may_build: bool,
    pub flying_speed: f32,
    pub walking_speed: f32,
}

impl PartialEq for PlayerAbilities {
    /// Compare snapshots as stored so an unchanged NaN does not send another abilities packet.
    fn eq(&self, other: &Self) -> bool {
        self.flags() == other.flags()
            && self.may_build == other.may_build
            && self.flying_speed.to_bits() == other.flying_speed.to_bits()
            && self.walking_speed.to_bits() == other.walking_speed.to_bits()
    }
}

impl Default for PlayerAbilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            may_fly: false,
            instabuild: false,
            may_build: true,
            flying_speed: 0.05,
            walking_speed: 0.1,
        }
    }
}

impl PlayerAbilities {
    /// `GameType.updatePlayerAbilities`: the flags of `mode` (0 survival, 1
    /// creative, 2 adventure, 3 spectator). Creative keeps `flying`; the
    /// speeds are not touched.
    pub fn update_for_mode(&mut self, mode: i32) {
        match mode {
            1 => {
                self.may_fly = true;
                self.instabuild = true;
                self.invulnerable = true;
            }
            3 => {
                self.may_fly = true;
                self.instabuild = false;
                self.invulnerable = true;
                self.flying = true;
            }
            _ => {
                self.may_fly = false;
                self.instabuild = false;
                self.invulnerable = false;
                self.flying = false;
            }
        }
        self.may_build = mode < 2;
    }

    /// The flags byte of `ClientboundPlayerAbilitiesPacket`.
    pub fn flags(&self) -> i8 {
        i8::from(self.invulnerable)
            | i8::from(self.flying) << 1
            | i8::from(self.may_fly) << 2
            | i8::from(self.instabuild) << 3
    }

    /// `ClientboundPlayerAbilitiesPacket(abilities)`.
    pub fn packet(&self) -> play::PlayerAbilities {
        play::PlayerAbilities {
            flags: self.flags(),
            flying_speed: self.flying_speed,
            walking_speed: self.walking_speed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_snapshots_distinguish_changes_to_the_packet_bits() {
        let snapshot = PlayerAbilities {
            flying_speed: -0.0,
            walking_speed: f32::from_bits(0x7fc0_0001),
            ..PlayerAbilities::default()
        };
        assert_eq!(
            snapshot,
            PlayerAbilities {
                walking_speed: f32::from_bits(0x7fc0_0001),
                ..snapshot
            }
        );
        assert_ne!(
            snapshot,
            PlayerAbilities {
                flying_speed: 0.0,
                ..snapshot
            }
        );
        assert_ne!(
            snapshot,
            PlayerAbilities {
                walking_speed: f32::from_bits(0x7fc0_0002),
                ..snapshot
            }
        );
    }

    #[test]
    fn a_mode_sets_the_flags_as_vanilla_does() {
        let mut a = PlayerAbilities::default();
        a.update_for_mode(1);
        assert_eq!(a.flags(), 1 | 4 | 8);
        a.flying = true;
        a.update_for_mode(1);
        assert!(a.flying, "creative keeps flying");
        a.update_for_mode(3);
        assert_eq!((a.flags(), a.may_build), (1 | 2 | 4, false));
        a.update_for_mode(2);
        assert_eq!((a.flags(), a.may_build), (0, false));
        a.walking_speed = 0.2;
        a.update_for_mode(0);
        assert_eq!((a.flags(), a.may_build, a.walking_speed), (0, true, 0.2));
    }
}
