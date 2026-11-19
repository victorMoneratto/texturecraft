//! Updating part of a set's aux texture: the tiles a stroke changed, without the rest of the image.

use super::{Renderer, aux_size};
use crate::aux_region::AuxRegion;

/// Bytes of an aux texel (RGBA8)
const TEXEL_BYTES: u32 = 4;

impl Renderer {
    /// Replace a rectangle of a set's aux texture and mark only that part of its mip chain
    /// stale, as [`Renderer::upload_tile`] does for a display texture. The whole image comes
    /// first, through [`Renderer::upload_aux`]: a region arriving before it, measured for
    /// another size or reaching outside the texture has nothing to update and is ignored.
    pub fn upload_aux_region(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texture_set: usize, region: &AuxRegion) {
        let Some(set) = self.sets.get_mut(texture_set) else { return };
        let side = aux_size(set.size);
        let aux_index = set.tex.len().saturating_sub(1);

        // Reject a rectangle that leaves the texture before a GPU origin can wrap around
        let (Some(right), Some(bottom)) = (region.x.checked_add(region.width), region.y.checked_add(region.height)) else { return };
        if region.side != side || region.width == 0 || region.height == 0 || right > side || bottom > side {
            return;
        }

        let needed_bytes = region.width as usize * region.height as usize * TEXEL_BYTES as usize;
        if region.rgba.len() < needed_bytes {
            return;
        }

        // A full-size texture WebGPU has not confirmed yet takes regions like the image before them
        let pending = set.pending.get(aux_index).and_then(Option::as_ref).map(|pending| &pending.tex);
        let Some(texture) = pending.or(set.tex.last()) else { return };
        if texture.texture.width() != side || texture.texture.height() != side {
            return;
        }

        // Error scopes contain the write if a pending allocation is rejected
        let out_of_memory_scope = pending.is_some().then(|| device.push_error_scope(wgpu::ErrorFilter::OutOfMemory));
        let validation_scope = pending.is_some().then(|| device.push_error_scope(wgpu::ErrorFilter::Validation));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: region.x, y: region.y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &region.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(region.width * TEXEL_BYTES), rows_per_image: Some(region.height) },
            wgpu::Extent3d { width: region.width, height: region.height, depth_or_array_layers: 1 },
        );

        // Drop the innermost error scope first
        drop(validation_scope);
        drop(out_of_memory_scope);
        self.content_rev += 1;
        if let Some(dirty) = set.mips_dirty.last_mut() {
            dirty.add(region.x, region.y, right, bottom);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::Headless;
    use crate::mips_dirty::MipsDirty;
    use crate::readback::Readback;
    use crate::renderer::mip_count;

    const SIDE: u32 = 256;
    const BLUE: [u8; 4] = [0, 0, 200, 255];
    const RED: [u8; 4] = [200, 0, 0, 255];

    /// A headless renderer with one 256² texture set
    fn renderer() -> Option<Headless> {
        let Ok(mut headless) = Headless::new() else {
            eprintln!("skipping: no GPU adapter");
            return None;
        };
        headless.renderer.set_texture_sets(&headless.device, &headless.queue, &[((SIDE, SIDE), 0)], 1);
        Some(headless)
    }

    fn upload_whole(headless: &mut Headless, colour: [u8; 4]) {
        headless.renderer.upload_aux(&headless.device, &headless.queue, 0, SIDE, &colour.repeat((SIDE * SIDE) as usize));
    }

    fn region(x: u32, y: u32, width: u32, height: u32, colour: [u8; 4]) -> AuxRegion {
        AuxRegion { side: SIDE, x, y, width, height, rgba: colour.repeat((width * height) as usize) }
    }

    fn rebuild_mips(headless: &mut Headless) {
        let mut encoder = headless.device.create_command_encoder(&Default::default());
        headless.renderer.generate_mips(&headless.device, &mut encoder);
        headless.queue.submit([encoder.finish()]);
    }

    /// Mip `level` of set 0's aux texture: its width and RGBA8
    fn aux_level(headless: &Headless, level: u32) -> (u32, Vec<u8>) {
        let texture = &headless.renderer.sets[0].tex.last().unwrap().texture;
        let (width, _, rgba) = Readback::blocking_level(&headless.device, &headless.queue, texture, level).unwrap();
        (width, rgba)
    }

    fn texel((width, rgba): &(u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
        let start = ((y * width + x) * 4) as usize;
        [rgba[start], rgba[start + 1], rgba[start + 2], rgba[start + 3]]
    }

    #[test]
    fn a_region_replaces_its_rectangle_and_leaves_the_rest() {
        let Some(mut headless) = renderer() else { return };
        upload_whole(&mut headless, BLUE);

        headless.renderer.upload_aux_region(&headless.device, &headless.queue, 0, &region(64, 128, 64, 32, RED));

        let image = aux_level(&headless, 0);
        for (x, y) in [(64, 128), (127, 159), (100, 140)] {
            assert_eq!(texel(&image, x, y), RED, "inside the region at {x}, {y}");
        }
        for (x, y) in [(63, 128), (128, 128), (64, 127), (64, 160), (0, 0), (255, 255)] {
            assert_eq!(texel(&image, x, y), BLUE, "outside the region at {x}, {y}");
        }
    }

    #[test]
    fn a_region_makes_only_its_part_of_the_mip_chain_stale() {
        let Some(mut headless) = renderer() else { return };
        upload_whole(&mut headless, BLUE);
        rebuild_mips(&mut headless);
        assert_eq!(headless.renderer.sets[0].mips_dirty.last(), Some(&MipsDirty::Clean));

        headless.renderer.upload_aux_region(&headless.device, &headless.queue, 0, &region(64, 128, 64, 64, RED));
        headless.renderer.upload_aux_region(&headless.device, &headless.queue, 0, &region(128, 128, 64, 64, RED));
        assert_eq!(headless.renderer.sets[0].mips_dirty.last(), Some(&MipsDirty::Region { x0: 64, y0: 128, x1: 192, y1: 192 }));

        // The partial rebuild gives every level what a rebuild of the whole chain gives
        rebuild_mips(&mut headless);
        assert_eq!(headless.renderer.sets[0].mips_dirty.last(), Some(&MipsDirty::Clean));
        let levels = |headless: &Headless| (1..mip_count(SIDE)).map(|level| aux_level(headless, level)).collect::<Vec<_>>();
        let partial = levels(&headless);
        assert_eq!(texel(&partial[0], 48, 80), RED, "the region reached the next level");
        assert_eq!(texel(&partial[0], 16, 16), BLUE);

        *headless.renderer.sets[0].mips_dirty.last_mut().unwrap() = MipsDirty::All;
        rebuild_mips(&mut headless);
        assert!(partial == levels(&headless), "a partial rebuild of the mips differs from a whole one");
    }

    #[test]
    fn a_region_before_the_whole_image_is_ignored() {
        let Some(mut headless) = renderer() else { return };
        let before = headless.renderer.content_rev();

        headless.renderer.upload_aux_region(&headless.device, &headless.queue, 0, &region(0, 0, 64, 64, RED));

        assert_eq!(headless.renderer.aux_size(0), Some(1), "the placeholder is not grown for a part of an image");
        assert_eq!(headless.renderer.content_rev(), before);
    }

    #[test]
    fn a_region_that_does_not_fit_the_texture_is_ignored() {
        let Some(mut headless) = renderer() else { return };
        upload_whole(&mut headless, BLUE);
        let before = headless.renderer.content_rev();

        let short = AuxRegion { rgba: vec![255; 16], ..region(0, 0, 64, 64, RED) };
        let other_size = AuxRegion { side: SIDE * 2, ..region(0, 0, 64, 64, RED) };
        let wrapping = AuxRegion { x: u32::MAX, ..region(0, 0, 64, 64, RED) };
        for unusable in [region(224, 0, 64, 64, RED), region(0, 224, 64, 64, RED), region(0, 0, 0, 64, RED), short, other_size, wrapping] {
            headless.renderer.upload_aux_region(&headless.device, &headless.queue, 0, &unusable);
            headless.renderer.upload_aux_region(&headless.device, &headless.queue, 7, &unusable);
        }

        assert_eq!(headless.renderer.content_rev(), before, "nothing was written");
        assert!(aux_level(&headless, 0).1.as_chunks::<4>().0.iter().all(|pixel| *pixel == BLUE));
    }
}
