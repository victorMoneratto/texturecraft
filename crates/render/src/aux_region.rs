/// A rectangle of a texture set's aux texture with new texels: what changed, uploaded without
/// the rest of the image (see [`crate::Renderer::upload_aux_region`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuxRegion {
    /// Side of the aux texture the rectangle was measured in ([`crate::aux_size`])
    pub side: u32,
    /// The rectangle's left and top texel
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width × height`, row by row
    pub rgba: Vec<u8>,
}
