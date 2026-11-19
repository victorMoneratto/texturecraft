//! Showing one colour over a whole set through its aux texture, without an image of it.

use super::{Renderer, install, neutral_tex};

impl Renderer {
    /// Make a set's aux texture one texel of `texel` (RGBA8), which stretches over the set like
    /// any aux image: a view of something the set does not have (the flow map of a set without
    /// the channel) then costs four bytes instead of an image of one colour, and a full-size aux
    /// texture left by an earlier view is released. The next [`Renderer::upload_aux`] grows it
    /// again.
    pub fn fill_aux(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texture_set: usize, texel: [u8; 4]) {
        let Some(set) = self.sets.get_mut(texture_set) else { return };
        let Some(aux_index) = set.tex.len().checked_sub(1) else { return };

        // A full-size texture WebGPU has not answered for is no longer wanted
        if let Some(pending) = set.pending.get_mut(aux_index) {
            *pending = None;
        }

        let one_texel = neutral_tex(device, queue, (1, 1), wgpu::TextureFormat::Rgba8Unorm, 1, &texel);
        install(set, aux_index, one_texel, device, &self.set_bgl, &self.sampler);
        self.content_rev += 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::headless::Headless;
    use crate::readback::Readback;

    const SIDE: u32 = 256;
    const STILL: [u8; 4] = [128, 128, 0, 255];
    const RED: [u8; 4] = [200, 0, 0, 255];

    fn renderer() -> Option<Headless> {
        let Ok(mut headless) = Headless::new() else {
            eprintln!("skipping: no GPU adapter");
            return None;
        };
        headless.renderer.set_texture_sets(&headless.device, &headless.queue, &[((SIDE, SIDE), 0)], 1);
        Some(headless)
    }

    /// Set 0's aux texture: its width and RGBA8
    fn aux(headless: &Headless) -> (u32, Vec<u8>) {
        let texture = &headless.renderer.sets[0].tex.last().unwrap().texture;
        let (width, _, rgba) = Readback::blocking_level(&headless.device, &headless.queue, texture, 0).unwrap();
        (width, rgba)
    }

    #[test]
    fn a_fill_is_one_texel_of_the_colour_and_releases_a_full_size_image() {
        let Some(mut headless) = renderer() else { return };
        headless.renderer.upload_aux(&headless.device, &headless.queue, 0, SIDE, &RED.repeat((SIDE * SIDE) as usize));
        assert_eq!(headless.renderer.aux_size(0), Some(SIDE));
        let before = headless.renderer.content_rev();

        headless.renderer.fill_aux(&headless.device, &headless.queue, 0, STILL);

        assert_eq!(aux(&headless), (1, STILL.to_vec()));
        assert!(headless.renderer.content_rev() > before, "the views are drawn again");

        // An image sent later grows the texture again
        headless.renderer.upload_aux(&headless.device, &headless.queue, 0, SIDE, &RED.repeat((SIDE * SIDE) as usize));
        let (width, rgba) = aux(&headless);
        assert_eq!(width, SIDE);
        assert!(rgba.as_chunks::<4>().0.iter().all(|texel| *texel == RED));
    }

    #[test]
    fn a_fill_of_a_set_that_does_not_exist_is_ignored() {
        let Some(mut headless) = renderer() else { return };
        let before = headless.renderer.content_rev();

        headless.renderer.fill_aux(&headless.device, &headless.queue, 7, STILL);

        assert_eq!(headless.renderer.content_rev(), before);
        assert_eq!(headless.renderer.aux_size(0), Some(1));
    }
}
