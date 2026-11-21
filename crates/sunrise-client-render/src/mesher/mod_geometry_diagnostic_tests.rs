use super::*;

#[test]
fn missing_sprite_diagnostic_is_bounded_without_limiting_valid_names() {
    let name = format!("bad\n\t{}", "界".repeat(350_000));
    let quad = SectionQuad {
        positions: [[0.0; 3]; 4],
        uvs: [[0.0; 2]; 4],
        color: 0,
        light: [0; 2],
        translucent: false,
    };
    let mut sprites = BTreeMap::new();
    let diagnostic = check_quad(&sprites, &name, &quad, 0).unwrap_err();
    assert!(diagnostic.len() < 320);
    assert!(diagnostic.contains('…'));
    assert!(!diagnostic.contains('\n'));
    assert!(!diagnostic.contains('\t'));

    // With vanilla's missingno sprite in the atlas, unknown sprites fall back to it.
    sprites.insert(MISSING_SPRITE.to_owned(), 3);
    assert_eq!(check_quad(&sprites, &name, &quad, 0), Ok(3));

    // Valid long sprite names resolve to their own layer without truncation.
    sprites.insert(name.clone(), 9);
    assert_eq!(check_quad(&sprites, &name, &quad, 0), Ok(9));

    // Atlas layers at or above OPAQUE_LAYER cannot fit in terrain vertex layer bits.
    sprites.insert(name.clone(), u32::from(OPAQUE_LAYER));
    let layer_err = check_quad(&sprites, &name, &quad, 0).unwrap_err();
    assert!(layer_err.len() < 320);
    assert!(layer_err.contains('…'));
}
