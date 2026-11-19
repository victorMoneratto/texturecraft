//! What a layer row shows for the blend mode of the displayed channel. The compositor decides
//! the mode a channel really blends in; a control that offered more than that would record a
//! choice that changes nothing.

use texturecraft_engine::compose::effective_blend;
use texturecraft_engine::core::{BlendMode, Channel};

use crate::i18n::n_;

/// Why the blend mode of a flow map cannot be picked: shown when the pointer rests on it
pub(crate) const FLOW_BLEND_HINT: &str = n_("A flow map always blends as Normal");

/// The mode a row shows for `channel` when `stored` is the mode its layer keeps for it: the one
/// the compositor blends with. With it, the reason the mode cannot be changed, where a channel
/// has one mode only.
pub(crate) fn shown_blend(channel: Channel, stored: BlendMode) -> (BlendMode, Option<&'static str>) {
    let effective = effective_blend(channel.kind(), stored);
    let every_mode_is_one = BlendMode::MENU.iter().flatten().all(|mode| effective_blend(channel.kind(), *mode) == effective);
    (effective, every_mode_is_one.then_some(FLOW_BLEND_HINT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flow_map_shows_normal_whatever_is_stored_and_says_why() {
        for stored in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Replace, BlendMode::Passthrough, BlendMode::Disable] {
            assert_eq!(shown_blend(Channel::Flow, stored), (BlendMode::Normal, Some(FLOW_BLEND_HINT)), "{stored:?}");
        }
    }

    #[test]
    fn other_channels_show_the_mode_they_store_and_can_change_it() {
        for channel in [Channel::BaseColor, Channel::Roughness, Channel::Height, Channel::Normal] {
            for stored in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Replace] {
                assert_eq!(shown_blend(channel, stored), (stored, None), "{channel:?} {stored:?}");
            }
        }
    }
}
