use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use texturecraft_core::Channel;
use texturecraft_paint::BrushSettings;

/// A brush the user saved: the whole brush (size included) and the channels it paints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserBrush {
    pub name: String,
    pub brush: BrushSettings,
    /// Channels painted when it was saved (None: leave the channels alone).
    #[serde(default)]
    pub channels: Option<BTreeSet<Channel>>,
    /// Whether its strokes painted the direction they travel into the flow map (None: leave
    /// that alone). Stored only by a brush that painted the flow map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_follow: Option<bool>,
    /// The file it was read from or last written to (None: not on disk yet). Deleting and
    /// renaming act on this file, which for an older bare `.tcbrush` is not the name's file.
    #[serde(skip)]
    pub file: Option<std::path::PathBuf>,
}
