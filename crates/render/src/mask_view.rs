//! The mask view's aux image for a texture set that has nothing of the mask to show: the mask
//! belongs to a layer of another set, or the layer has no enabled mask.

use crate::aux_size;

/// RGBA of the texel such a set shows: a flat dark gray
pub const MASK_VIEW_BLANK: [u8; 4] = [38, 38, 38, 255];

/// The whole blank aux image of a set of `set_size` texels, RGBA8.
pub fn mask_view_blank_rgba(set_size: (u32, u32)) -> Vec<u8> {
    let side = aux_size(set_size) as usize;
    MASK_VIEW_BLANK.repeat(side * side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blank_image_fills_the_aux_texture_and_stays_bounded() {
        let small = mask_view_blank_rgba((128, 64));
        assert_eq!(small.len(), 128 * 128 * 4);
        assert!(small.as_chunks::<4>().0.iter().all(|texel| *texel == MASK_VIEW_BLANK));

        // A hostile size is capped like every aux image
        assert_eq!(mask_view_blank_rgba((u32::MAX, 7)).len(), 2048 * 2048 * 4);
        assert_eq!(mask_view_blank_rgba((0, 0)).len(), 4);
    }
}
