//! Material channels of a texture set.

use serde::{Deserialize, Serialize};

/// A material channel. Order is the display order in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Channel {
    BaseColor,
    Metallic,
    Roughness,
    Normal,
    Height,
    Emissive,
    Opacity,
    AmbientOcclusion,
    SpecularLevel,
    /// Specular/glossiness workflow: the F0 color.
    Specular,
    /// Specular/glossiness workflow: 1 − roughness.
    Glossiness,
    /// Clear coat: weight of a GGX dielectric layer (IOR 1.5) over the base.
    CoatWeight,
    /// Clear coat roughness.
    CoatRoughness,
    /// Sheen (cloth fuzz) layer weight.
    SheenWeight,
    /// Sheen tint.
    SheenColor,
    /// Sheen roughness.
    SheenRoughness,
    /// Anisotropy: how much the specular highlight stretches along the tangent direction.
    AnisotropyLevel,
    /// Anisotropy direction: 0..1 turns the tangent by 0..π.
    AnisotropyAngle,
    /// Subsurface scattering weight (scatter color and radius are project shader settings).
    ScatterWeight,
    /// Transmission: how much of the dielectric base lets light through (refraction at the
    /// project's IOR, tinted by the base color) instead of reflecting it diffusely.
    Transmission,
    /// Tangent-space normal of the clear coat (relative to the mesh normal).
    CoatNormal,
    /// Flow map: the direction the surface appears to move, in the mesh's UV basis (u right,
    /// v down), times a 0..1 speed. No shading lobe reads it: it is authored for export.
    Flow,
}

/// A flow map texel with no flow: a zero vector, packed.
pub const FLOW_STILL: [f32; 3] = [0.5, 0.5, 0.0];

/// The packed flow map value of a flow at `angle_degrees` and `speed` (0..1). The angle turns
/// counter-clockwise as the texture is seen, from 0 pointing right: 90 points up the image,
/// which is towards smaller V. Hostile numbers give a still texel.
pub fn flow_from_polar(angle_degrees: f32, speed: f32) -> [f32; 3] {
    if !angle_degrees.is_finite() || !speed.is_finite() {
        return FLOW_STILL;
    }

    let (sin, cos) = angle_degrees.to_radians().sin_cos();
    let speed = speed.clamp(0.0, 1.0);
    [0.5 + 0.5 * cos * speed, 0.5 - 0.5 * sin * speed, 0.0]
}

/// The angle (0..360 degrees, see [`flow_from_polar`]) and speed (0..1) of a packed flow map
/// value. A still texel has no direction and reads as angle 0.
pub fn flow_to_polar(value: [f32; 3]) -> (f32, f32) {
    let (x, y) = (value[0] * 2.0 - 1.0, value[1] * 2.0 - 1.0);
    let speed = x.hypot(y);
    if !speed.is_finite() || speed < FLOW_MIN_SPEED {
        return (0.0, 0.0);
    }

    ((-y).atan2(x).to_degrees().rem_euclid(360.0), speed.min(1.0))
}

/// Below this length a flow vector has no usable direction (16-bit storage keeps about 1/32768)
const FLOW_MIN_SPEED: f32 = 1e-4;

/// How a channel's values are stored and interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    /// Linear RGB color, displayed and exported sRGB-encoded.
    Color,
    /// Tangent-space normal packed into 0..1 (`n * 0.5 + 0.5`).
    NormalMap,
    /// A 0..1 scalar.
    Scalar,
    /// A signed scalar (height).
    Signed,
    /// A signed 2D vector packed into 0..1 (`v * 0.5 + 0.5`) in the first two components; the
    /// third is unused and zero. Unlike a normal it has a length, so it is never renormalised.
    Vector,
}

impl Channel {
    pub const ALL: [Channel; 22] = [
        Channel::BaseColor,
        Channel::Metallic,
        Channel::Roughness,
        Channel::Normal,
        Channel::Height,
        Channel::Emissive,
        Channel::Opacity,
        Channel::AmbientOcclusion,
        Channel::SpecularLevel,
        Channel::Specular,
        Channel::Glossiness,
        Channel::CoatWeight,
        Channel::CoatRoughness,
        Channel::SheenWeight,
        Channel::SheenColor,
        Channel::SheenRoughness,
        Channel::AnisotropyLevel,
        Channel::AnisotropyAngle,
        Channel::ScatterWeight,
        Channel::Transmission,
        Channel::CoatNormal,
        // Appended, never inserted: the viewport shaders switch on these indices
        Channel::Flow,
    ];

    /// The extra shading lobes (clear coat, sheen, anisotropy, subsurface scattering,
    /// transmission). Their defaults switch the lobes off, so a set without them shades exactly
    /// as before.
    pub const LOBES: [Channel; 10] = [
        Channel::CoatWeight,
        Channel::CoatRoughness,
        Channel::SheenWeight,
        Channel::SheenColor,
        Channel::SheenRoughness,
        Channel::AnisotropyLevel,
        Channel::AnisotropyAngle,
        Channel::ScatterWeight,
        Channel::Transmission,
        Channel::CoatNormal,
    ];

    /// The channels of the default "PBR - Metallic Roughness" template.
    pub const DEFAULT_SET: [Channel; 5] = [Channel::BaseColor, Channel::Metallic, Channel::Roughness, Channel::Normal, Channel::Height];

    pub fn kind(self) -> ChannelKind {
        match self {
            Channel::BaseColor | Channel::Emissive | Channel::Specular | Channel::SheenColor => ChannelKind::Color,
            Channel::Normal | Channel::CoatNormal => ChannelKind::NormalMap,
            Channel::Height => ChannelKind::Signed,
            Channel::Flow => ChannelKind::Vector,
            _ => ChannelKind::Scalar,
        }
    }

    /// Value components (without coverage): 3 for colors, normals and vectors, 1 for scalars.
    pub fn components(self) -> usize {
        match self.kind() {
            ChannelKind::Color | ChannelKind::NormalMap | ChannelKind::Vector => 3,
            ChannelKind::Scalar | ChannelKind::Signed => 1,
        }
    }

    /// The value a texel has when no layer covers it.
    pub fn default_value(self) -> [f32; 3] {
        match self {
            Channel::BaseColor => [0.5, 0.5, 0.5],
            Channel::Metallic | Channel::Height | Channel::Emissive => [0.0; 3],
            Channel::CoatWeight
            | Channel::SheenWeight
            | Channel::AnisotropyLevel
            | Channel::AnisotropyAngle
            | Channel::ScatterWeight
            | Channel::Transmission => [0.0; 3],
            Channel::CoatRoughness => [0.1; 3],
            Channel::SheenColor => [1.0; 3],
            Channel::SheenRoughness => [0.5; 3],
            Channel::Roughness => [0.5; 3],
            Channel::Normal | Channel::CoatNormal => [0.5, 0.5, 1.0],
            Channel::Flow => FLOW_STILL,
            Channel::Opacity | Channel::AmbientOcclusion => [1.0; 3],
            Channel::SpecularLevel | Channel::Glossiness => [0.5; 3],
            // F0 of a common dielectric (4 %).
            Channel::Specular => [0.04; 3],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Channel::BaseColor => "Base color",
            Channel::Metallic => "Metallic",
            Channel::Roughness => "Roughness",
            Channel::Normal => "Normal",
            Channel::Height => "Height",
            Channel::Emissive => "Emissive",
            Channel::Opacity => "Opacity",
            Channel::AmbientOcclusion => "Ambient occlusion",
            Channel::SpecularLevel => "Specular level",
            Channel::Specular => "Specular",
            Channel::Glossiness => "Glossiness",
            Channel::CoatWeight => "Coat weight",
            Channel::CoatRoughness => "Coat roughness",
            Channel::SheenWeight => "Sheen weight",
            Channel::SheenColor => "Sheen color",
            Channel::SheenRoughness => "Sheen roughness",
            Channel::AnisotropyLevel => "Anisotropy level",
            Channel::AnisotropyAngle => "Anisotropy angle",
            Channel::ScatterWeight => "Scattering weight",
            Channel::Transmission => "Transmission",
            Channel::CoatNormal => "Coat normal",
            // Not "Flow": the brush already has a setting of that name
            Channel::Flow => "Flow map",
        }
    }

    /// Short label used on material channel toggle buttons.
    pub fn short(self) -> &'static str {
        match self {
            Channel::BaseColor => "color",
            Channel::Metallic => "metal",
            Channel::Roughness => "rough",
            Channel::Normal => "nrm",
            Channel::Height => "height",
            Channel::Emissive => "emiss",
            Channel::Opacity => "op",
            Channel::AmbientOcclusion => "ao",
            Channel::SpecularLevel => "spec",
            Channel::Specular => "specular",
            Channel::Glossiness => "gloss",
            Channel::CoatWeight => "coat",
            Channel::CoatRoughness => "coat rough",
            Channel::SheenWeight => "sheen",
            Channel::SheenColor => "sheen color",
            Channel::SheenRoughness => "sheen rough",
            Channel::AnisotropyLevel => "aniso",
            Channel::AnisotropyAngle => "aniso angle",
            Channel::ScatterWeight => "scatter",
            Channel::Transmission => "transm",
            Channel::CoatNormal => "coat nrm",
            Channel::Flow => "flow map",
        }
    }

    /// Stable machine id (commands, export templates, file names).
    pub fn id(self) -> &'static str {
        match self {
            Channel::BaseColor => "baseColor",
            Channel::Metallic => "metallic",
            Channel::Roughness => "roughness",
            Channel::Normal => "normal",
            Channel::Height => "height",
            Channel::Emissive => "emissive",
            Channel::Opacity => "opacity",
            Channel::AmbientOcclusion => "ambientOcclusion",
            Channel::SpecularLevel => "specularLevel",
            Channel::Specular => "specular",
            Channel::Glossiness => "glossiness",
            Channel::CoatWeight => "coatWeight",
            Channel::CoatRoughness => "coatRoughness",
            Channel::SheenWeight => "sheenWeight",
            Channel::SheenColor => "sheenColor",
            Channel::SheenRoughness => "sheenRoughness",
            Channel::AnisotropyLevel => "anisotropyLevel",
            Channel::AnisotropyAngle => "anisotropyAngle",
            Channel::ScatterWeight => "scatterWeight",
            Channel::Transmission => "transmission",
            Channel::CoatNormal => "coatNormal",
            Channel::Flow => "flow",
        }
    }

    /// Parse an id, label or short label (case-insensitive, spaces/underscores ignored).
    pub fn parse(s: &str) -> Option<Channel> {
        let norm = |x: &str| x.to_ascii_lowercase().replace([' ', '_', '-'], "");
        let want = norm(s);
        Channel::ALL.into_iter().find(|c| norm(c.id()) == want || norm(c.label()) == want || norm(c.short()) == want || norm(&format!("{c:?}")) == want)
    }

    pub fn index(self) -> usize {
        Channel::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// Storage format of a channel in a texture set (what export and the UI report).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelFormat {
    Srgb8,
    L8,
    Rgb8,
    L16,
    Rgb16,
    L16F,
    Rgb16F,
    L32F,
    Rgb32F,
}

impl ChannelFormat {
    pub fn default_for(c: Channel) -> ChannelFormat {
        match c.kind() {
            ChannelKind::Color => ChannelFormat::Srgb8,
            ChannelKind::NormalMap => ChannelFormat::Rgb16F,
            ChannelKind::Vector => ChannelFormat::Rgb16,
            ChannelKind::Scalar => ChannelFormat::L8,
            ChannelKind::Signed => ChannelFormat::L16F,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ChannelFormat::Srgb8 => "sRGB8",
            ChannelFormat::L8 => "L8",
            ChannelFormat::Rgb8 => "RGB8",
            ChannelFormat::L16 => "L16",
            ChannelFormat::Rgb16 => "RGB16",
            ChannelFormat::L16F => "L16F",
            ChannelFormat::Rgb16F => "RGB16F",
            ChannelFormat::L32F => "L32F",
            ChannelFormat::Rgb32F => "RGB32F",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_ids_labels_and_shorts() {
        for c in Channel::ALL {
            assert_eq!(Channel::parse(c.id()), Some(c));
            assert_eq!(Channel::parse(c.label()), Some(c));
            assert_eq!(Channel::parse(c.short()), Some(c));
        }
        assert_eq!(Channel::parse("base color"), Some(Channel::BaseColor));
        assert_eq!(Channel::parse("nope"), None);
    }

    #[test]
    fn names_are_unique_and_lobes_default_off() {
        let norm = |x: &str| x.to_ascii_lowercase().replace([' ', '_', '-'], "");
        let mut seen = std::collections::BTreeMap::new();
        for c in Channel::ALL {
            for name in [c.id(), c.label(), c.short()] {
                let prev = seen.insert(norm(name), c);
                assert!(prev.is_none_or(|p| p == c), "`{name}` names both {prev:?} and {c:?}");
            }
        }
        for c in Channel::LOBES {
            assert!(Channel::ALL.contains(&c));
            // Tints, roughnesses and the flat coat normal only matter once their lobe has weight.
            let off =
                matches!(c, Channel::CoatRoughness | Channel::SheenColor | Channel::SheenRoughness | Channel::CoatNormal) || c.default_value() == [0.0; 3];
            assert!(off, "{c:?} must default to an inactive lobe");
        }
        assert_eq!(Channel::parse("sheen color"), Some(Channel::SheenColor));
        assert_eq!(Channel::parse("coatWeight"), Some(Channel::CoatWeight));
    }

    #[test]
    fn the_flow_map_is_a_vector_appended_after_the_shader_indexed_channels() {
        // `kind()` ends in a catch-all, so nothing but this test notices a missing arm
        assert_eq!(Channel::Flow.kind(), ChannelKind::Vector);
        assert_eq!(Channel::Flow.components(), 3);
        assert_eq!(Channel::Flow.default_value(), FLOW_STILL);
        assert_eq!(Channel::ALL.last(), Some(&Channel::Flow));
        assert_eq!(Channel::CoatNormal.index(), 20);
        assert!(!Channel::DEFAULT_SET.contains(&Channel::Flow));
        assert!(!Channel::LOBES.contains(&Channel::Flow));
        assert_eq!(Channel::parse("flow"), Some(Channel::Flow));
        assert_eq!(Channel::parse("Flow map"), Some(Channel::Flow));
    }

    #[test]
    fn flow_angles_turn_counter_clockwise_on_the_image_and_round_trip() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);

        // Right is +U; up the image is towards smaller V
        assert!(close(flow_from_polar(0.0, 1.0), [1.0, 0.5, 0.0]));
        assert!(close(flow_from_polar(90.0, 1.0), [0.5, 0.0, 0.0]));
        assert!(close(flow_from_polar(180.0, 0.5), [0.25, 0.5, 0.0]));
        assert!(close(flow_from_polar(45.0, 0.0), FLOW_STILL));

        for (angle, speed) in [(0.0, 1.0), (37.0, 0.25), (135.0, 0.8), (270.0, 0.6), (359.0, 1.0)] {
            let (a, s) = flow_to_polar(flow_from_polar(angle, speed));
            assert!((a - angle).abs() < 1e-3 && (s - speed).abs() < 1e-5, "{angle} {speed} -> {a} {s}");
        }
        assert_eq!(flow_to_polar(FLOW_STILL), (0.0, 0.0));

        // Speed is clamped and hostile numbers paint nothing
        assert!(close(flow_from_polar(0.0, 7.0), [1.0, 0.5, 0.0]));
        assert_eq!(flow_from_polar(f32::NAN, 1.0), FLOW_STILL);
        assert_eq!(flow_from_polar(0.0, f32::INFINITY), FLOW_STILL);
    }

    #[test]
    fn index_matches_all_order() {
        for (i, c) in Channel::ALL.iter().enumerate() {
            assert_eq!(c.index(), i);
        }
    }
}
