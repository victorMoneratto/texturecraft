//! Which shader mode and aux texture content the raw flow map view asks for

use texturecraft_engine::geom::primitives;

use super::*;
use crate::gpu::AuxView;
use crate::state::Mode;

fn app_showing(view: ViewMode) -> egui_kittest::Harness<'static, TextureCraftApp> {
    let session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, 128).unwrap();
    let mut harness = egui_kittest::Harness::builder().build_eframe(|context| TextureCraftApp::new(context, session));
    let app = harness.state_mut();
    app.ui.mode = Mode::Painting;
    app.session.view = view;
    harness
}

#[test]
fn the_flow_map_view_reads_the_aux_texture_filled_with_the_flow_map() {
    let _language = crate::i18n::TEST_LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let harness = app_showing(ViewMode::Channel(Channel::Flow));
    let app = harness.state();

    assert_eq!(shade(app).view_mode, VIEW_MODE_AUX_TEXTURE);
    assert_eq!(aux_view(app, Mode::Painting), Some(AuxView::Flow));

    // Bake mode's bent normal preview uses the aux texture too: the chosen view comes first
    assert_eq!(aux_view(app, Mode::Baking), Some(AuxView::Flow));
}

#[test]
fn the_other_channels_keep_their_display_textures() {
    let _language = crate::i18n::TEST_LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for channel in Channel::ALL.into_iter().filter(|channel| *channel != Channel::Flow) {
        let harness = app_showing(ViewMode::Channel(channel));
        let app = harness.state();

        let shade = shade(app);
        assert_eq!((shade.view_mode, shade.channel), (VIEW_MODE_CHANNEL, channel.index() as u32), "{channel:?}");
        assert_eq!(aux_view(app, Mode::Painting), None, "{channel:?}");
    }
}

#[test]
fn the_quick_mask_replaces_the_flow_map_view_while_it_is_edited() {
    let _language = crate::i18n::TEST_LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut harness = app_showing(ViewMode::Channel(Channel::Flow));
    let app = harness.state_mut();

    app.session.quick_mask.editing = true;
    assert_eq!(aux_view(app, Mode::Painting), Some(AuxView::QuickMask));
    assert_eq!(shade(app).view_mode, VIEW_MODE_AUX_TEXTURE);

    app.session.quick_mask.editing = false;
    assert_eq!(aux_view(app, Mode::Painting), Some(AuxView::Flow), "the flow map view returns");
}
