#[doc = r#"a firework's explosions drawn on this client (`create_fireworks`), with or without their sound."#]
pub trait Notes {
    #[notes = r#"`ClientHost::create_fireworks(&FireworksRequest) -> bool` draws a firework's explosions at a point in this client's world through the same starter a bursting rocket uses: each `FireworkExplosion`'s shape, colours, fade colours, trail and twinkle, and the rocket's velocity carried by the sparks. `audible` gates the blast and twinkle sounds, as a rocket's silence does; the sparks and flash draw either way. No explosions puffs a little smoke, as vanilla's. The method returns false on servers, without a client player/level, or for non-finite input. The 256-explosion limit applies to the `minecraft:fireworks` item component, not to this visual list. Bundles need "hooks": @API:client_fireworks@."#]
    fn client_fireworks();
}
