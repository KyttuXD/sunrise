//! The GUI atlas's animated sprites, frame by frame (vanilla's or a Polytone `mode`'s).

impl super::HudRenderer {
    /// Shows each animated sprite's frame: its mcmeta's, or what its Polytone `mode` picks.
    pub(crate) fn animate(
        &mut self,
        uploads: &mut crate::gfx::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        tick: u64,
        animator: &crate::gfx::animate::SpriteAnimator,
    ) {
        let inputs = animator.mode_inputs();
        let inputs = Some(&inputs);
        for (texture, animations) in self
            .atlas_texture
            .as_ref()
            .zip(Some(&mut self.sprite_animations))
            .into_iter()
            .chain(
                self.atlas_pages
                    .iter_mut()
                    .flatten()
                    .map(|p| (&p.page.texture, &mut p.page.animations)),
            )
        {
            for (rect, animation, previous) in animations {
                // `SpriteContents.AnimationState`: the frame from the mcmeta's
                // list, blended into the next one by the sub-frame's thousandths
                // when the sprite interpolates; or what a Polytone `mode` picks.
                let blend = animation.blend_with(tick, inputs);
                let state = (blend.frame, blend.next, blend.millis);
                if state == *previous {
                    continue;
                }
                let (w, h) = rect.texels((texture.width(), texture.height()));
                let Ok(mut pixels) = animation.resized_pixels(blend.frame, w, h) else {
                    continue;
                };
                if blend.millis != 0 {
                    let Ok(next) = animation.resized_pixels(blend.next, w, h) else {
                        continue;
                    };
                    let millis = blend.millis;
                    for (a, b) in pixels.iter_mut().zip(next) {
                        *a = ((u32::from(*a) * (1000 - millis) + u32::from(b) * millis + 500)
                            / 1000) as u8;
                    }
                }
                uploads.texture(
                    encoder,
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: (rect.u0 * texture.width() as f32).round() as u32,
                            y: (rect.v0 * texture.height() as f32).round() as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    w,
                    h,
                    &pixels,
                );
                *previous = state;
            }
        }
    }
}
