//! The parts of the UI state that only this build understands, kept where the released build
//! does not look.
//!
//! Both builds share one settings file. The released build parses the whole of it with its own
//! types and starts from default preferences and panels when one value does not parse, so a
//! value it has no name for (the flow map channel) is never written where it expects one of
//! its own. This build keeps such a value under a key of its own, which the released build
//! passes over like any key it does not know.

use serde::{Deserialize, Serialize, Serializer};
use texturecraft_engine::core::Channel;

use crate::UiState;

/// The channel the released build is given where this build shows one it lacks
const RELEASED_FALLBACK: Channel = Channel::BaseColor;

/// Whether the released build has `channel`. Its channels are this build's but the flow map
pub(crate) fn released_build_has(channel: Channel) -> bool {
    channel != Channel::Flow
}

/// Serialise a channel in a field the released build parses: `channel` when it has it, else a
/// channel it does have. Used by the field itself, so no way of saving the state can skip it
pub(crate) fn serialize_for_released_build<S: Serializer>(channel: &Channel, serializer: S) -> Result<S::Ok, S::Error> {
    if released_build_has(*channel) { channel } else { &RELEASED_FALLBACK }.serialize(serializer)
}

/// What [`UiState`] holds that the released build cannot read, saved beside it under keys of
/// this build's own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ForkOnlyUi {
    /// The channel of the layer rows, when the released build has no such channel
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer_channel_fork: Option<Channel>,
}

impl ForkOnlyUi {
    /// The values of `ui` that its own fields save as something else
    pub(crate) fn of(ui: &UiState) -> ForkOnlyUi {
        ForkOnlyUi { layer_channel_fork: (!released_build_has(ui.layer_channel)).then_some(ui.layer_channel) }
    }

    /// The values saved beside a UI state in `saved` (JSON). Absent or unreadable ones are none:
    /// the state then keeps what the released build was given
    pub(crate) fn read(saved: &[u8]) -> ForkOnlyUi {
        serde_json::from_slice(saved).unwrap_or_default()
    }

    /// Put the values back into the state they were saved beside
    pub(crate) fn restore(self, ui: &mut UiState) {
        if let Some(channel) = self.layer_channel_fork {
            ui.layer_channel = channel;
        }
    }
}

/// A UI state with its fork-only values, as one flat JSON object
#[derive(Serialize)]
pub(crate) struct SavedUi<'a> {
    #[serde(flatten)]
    ui: &'a UiState,
    #[serde(flatten)]
    fork_only: ForkOnlyUi,
}

impl<'a> SavedUi<'a> {
    pub(crate) fn of(ui: &'a UiState) -> SavedUi<'a> {
        SavedUi { ui, fork_only: ForkOnlyUi::of(ui) }
    }
}
