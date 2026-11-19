//! What this build saves stays readable by the released build, which has no flow map channel:
//! it parses the whole UI state with its own `Channel` and starts from defaults when that fails

use serde::Deserialize;
use texturecraft_engine::core::Channel;

use super::memory_storage::MemoryStorage;
use super::*;

/// The channels of the released build, by the names its settings are parsed with
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
enum ReleasedChannel {
    BaseColor,
    Metallic,
    Roughness,
    Normal,
    Height,
    Emissive,
    Opacity,
    AmbientOcclusion,
    SpecularLevel,
    Specular,
    Glossiness,
    CoatWeight,
    CoatRoughness,
    SheenWeight,
    SheenColor,
    SheenRoughness,
    AnisotropyLevel,
    AnisotropyAngle,
    ScatterWeight,
    Transmission,
    CoatNormal,
}

/// The part of the released build's UI state that holds a channel. Like the whole of it, it
/// passes over keys it does not know
#[derive(Debug, Deserialize)]
struct ReleasedUiState {
    layer_channel: ReleasedChannel,
}

/// The key under which this build keeps a layer rows channel the released build lacks
const FORK_KEY: &str = "layer_channel_fork";

fn saved_text(ui: &UiState) -> (MemoryStorage, String) {
    let mut storage = MemoryStorage::default();
    UiStateStore::default().save(ui, &mut storage);
    let text = storage.values.get(STORAGE_KEY).cloned().expect("the state is saved");
    (storage, text)
}

/// Every place in `value` that names the flow map, as key paths
fn flow_mentions(value: &serde_json::Value, path: &str, found: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) if text == "Flow" => found.push(path.to_string()),
        serde_json::Value::Array(items) => items.iter().enumerate().for_each(|(index, item)| flow_mentions(item, &format!("{path}[{index}]"), found)),
        serde_json::Value::Object(fields) => {
            for (key, field) in fields {
                if key == "Flow" {
                    found.push(format!("{path}.{key} (a key)"));
                }
                flow_mentions(field, &format!("{path}.{key}"), found);
            }
        }
        _ => {}
    }
}

#[test]
fn the_released_build_reads_settings_saved_with_the_flow_map_in_the_layer_rows() {
    let ui = UiState { layer_channel: Channel::Flow, ..UiState::default() };

    let (storage, text) = saved_text(&ui);

    // Where the released build expects one of its channels there is one
    let released: ReleasedUiState = serde_json::from_str(&text).expect("the released build parses the saved state");
    assert_eq!(released.layer_channel, ReleasedChannel::BaseColor);

    // Nothing else in the file names the channel, but this build's own key
    let mut mentions = Vec::new();
    flow_mentions(&serde_json::from_str(&text).unwrap(), "", &mut mentions);
    assert_eq!(mentions, [format!(".{FORK_KEY}")]);

    // This build shows the flow map again after a restart
    let reopened = UiStateStore::default().load(Some(&storage)).expect("the saved state loads");
    assert_eq!(reopened.layer_channel, Channel::Flow);
}

#[test]
fn the_settings_file_on_disk_is_readable_by_the_released_build_too() {
    let folder = std::env::temp_dir().join(format!("texturecraft-ui-released-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("ui.json");
    let ui = UiState { layer_channel: Channel::Flow, ..UiState::default() };

    UiStateStore::new(Some(file.clone())).save(&ui, &mut MemoryStorage::default());

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(serde_json::from_str::<ReleasedUiState>(&text).is_ok(), "{text}");
    assert_eq!(read(&file).map(|ui| ui.layer_channel), Some(Channel::Flow));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_channel_both_builds_have_is_saved_as_before() {
    for channel in Channel::ALL.into_iter().filter(|channel| *channel != Channel::Flow) {
        let ui = UiState { layer_channel: channel, ..UiState::default() };

        let (storage, text) = saved_text(&ui);

        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["layer_channel"], serde_json::json!(format!("{channel:?}")));
        assert!(saved.get(FORK_KEY).is_none(), "{channel:?}: no key the released build does not need");
        assert!(serde_json::from_str::<ReleasedUiState>(&text).is_ok(), "{channel:?}");
        assert_eq!(UiStateStore::default().load(Some(&storage)).map(|ui| ui.layer_channel), Some(channel));
    }
}

#[test]
fn a_state_written_without_the_store_still_holds_no_channel_the_released_build_lacks() {
    let ui = UiState { layer_channel: Channel::Flow, ..UiState::default() };

    // The control channel, a test or a future caller serialising the state on its own
    let text = serde_json::to_string(&ui).unwrap();

    assert!(serde_json::from_str::<ReleasedUiState>(&text).is_ok(), "{text}");
    let mut mentions = Vec::new();
    flow_mentions(&serde_json::from_str(&text).unwrap(), "", &mut mentions);
    assert_eq!(mentions, Vec::<String>::new());
}

#[test]
fn a_damaged_fork_key_is_passed_over() {
    let ui = UiState { layer_channel: Channel::Roughness, ..UiState::default() };
    let (_, text) = saved_text(&ui);

    for damaged in [serde_json::json!("NoSuchChannel"), serde_json::json!(7), serde_json::json!({"Flow": 1}), serde_json::Value::Null] {
        let mut saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        saved[FORK_KEY] = damaged.clone();
        let mut storage = MemoryStorage::default();
        storage.values.insert(STORAGE_KEY.to_string(), saved.to_string());

        let loaded = UiStateStore::default().load(Some(&storage)).expect("the rest of the state loads");
        assert_eq!(loaded.layer_channel, Channel::Roughness, "{damaged}");
    }
}
