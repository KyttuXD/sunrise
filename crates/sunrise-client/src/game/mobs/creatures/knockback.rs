use super::*;

impl Game<'_> {
    /// `LivingEntity.knockback` on the player: push away from (`xd`, `zd`), with the
    /// living knockback hook before resistance and motion changes. Unlike some mobs,
    /// players apply damage pushes and effect pushes the same way.
    pub(in crate::game) fn knock_back_player(
        &mut self,
        power: f64,
        xd: f64,
        zd: f64,
        source: HurtSource<'_>,
        attack: &Attack,
        _damage: f32,
    ) {
        // No attacker named: the mob the player was last hurt by.
        let credited;
        let attack = if attack.attacker == 0
            && let Some((Who::Mob(id), _)) = self.player_hurt_by()
        {
            credited = self.attack_of(id as i32);
            &credited
        } else {
            attack
        };
        let plan = self.prepare_knockback_credit(
            self.player.as_ref().map_or(0, |p| p.entity_id),
            None,
            attack,
        );
        let mut credit = plan.lend(&mut self.last_hurt_by_player);
        if let Some(p) = self.player.as_mut() {
            let level = crate::game::entity::damage_level_of!(self, attack, &mut credit, None);
            apply_player_knockback(p, level, power, (xd, zd), source);
        }
    }
}

/// Apply the full entry's physical push.
pub(super) fn apply_player_knockback(
    p: &mut sunrise_client::motion::PlayerMotion,
    level: &mut sunrise_client::health::DamageLevel<'_>,
    power: f64,
    (xd, zd): (f64, f64),
    source: HurtSource<'_>,
) {
    let resistance = p.attribute("minecraft:knockback_resistance").unwrap_or(0.0);
    // The native hook runs first, on the raw strength (`LivingKnockBackEvent`), then the
    // resistance scales what it left.
    let mut knock = Knock {
        strength: power,
        x: xd,
        z: zd,
    };
    if level.knockback_canceled(Hurting::Player(p.entity_id, source), &mut knock) {
        return;
    }
    let Knock {
        strength: power,
        x: mut xd,
        z: mut zd,
    } = knock;
    let power = power * (1.0 - resistance);
    if power <= 0.0 {
        return;
    }
    while xd * xd + zd * zd < f64::from(1.0e-5_f32) {
        let random = level.random();
        xd = (random.next_double() - random.next_double()) * 0.01;
        zd = (random.next_double() - random.next_double()) * 0.01;
    }
    let l = (xd * xd + zd * zd).sqrt();
    let v = p.velocity;
    p.velocity = Vec3::new(
        v.x / 2.0 - xd / l * power,
        if p.ground.on_ground {
            (v.y / 2.0 + power).min(0.4)
        } else {
            v.y
        },
        v.z / 2.0 - zd / l * power,
    );
}
