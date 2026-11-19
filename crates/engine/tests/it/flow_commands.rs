//! The flow map's command surface: a refused request changes nothing, a malformed parameter is
//! refused and not skipped, partial edits keep what they do not name, and the tool state and
//! the inspect report show what a command changed

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL, LayerId, Source, flow_from_polar};
use texturecraft_engine::{Session, execute};

use super::flow_map::flow_session;

fn tool_state(session: &mut Session) -> Value {
    execute(session, "tool.state", &Value::Null).unwrap()
}

fn inspected_tool(session: &mut Session) -> Value {
    execute(session, "project.inspect", &Value::Null).unwrap()["tool"].clone()
}

fn material(session: &mut Session, params: Value) -> Value {
    execute(session, "tool.material", &params).unwrap()
}

fn brush_flow(session: &Session) -> [f32; 3] {
    session.tool.values[&Channel::Flow]
}

/// A fill layer that flows to the right at half speed, selected
fn flow_fill(session: &mut Session) -> u64 {
    let fill = execute(session, "layer.add.fill", &json!({"color": [0.5, 0.5, 0.5]})).unwrap()["layer"].as_u64().unwrap();
    execute(session, "layer.fill.set", &json!({"layer": fill, "channel": "flow", "angle": 0.0, "speed": 0.5})).unwrap();
    fill
}

fn fill_flow(session: &Session, fill: u64) -> [f32; 3] {
    match &session.doc.project.layer(LayerId(fill)).unwrap().kind {
        texturecraft_engine::core::LayerKind::Fill(fill) => match fill.sources.get(&Channel::Flow) {
            Some(Source::Uniform(value)) => *value,
            other => panic!("the fill's flow source is {other:?}"),
        },
        _ => panic!("not a fill layer"),
    }
}

/// Requests that each put one bad parameter among good ones, with the parameter that is bad
fn rejected_material_requests() -> Vec<(Value, &'static str)> {
    vec![
        (json!({"channel": "roughness", "angle": 30, "maskValue": 0, "material": "steel", "value": 0.9, "solo": true}), "angle"),
        (json!({"channel": "baseColor", "speed": 0.5, "color": [1, 0, 0], "maskValue": 0.2, "enabled": false}), "speed"),
        (json!({"channel": "flow", "angle": "north", "speed": 0.2, "maskValue": 0.1, "follow": false, "solo": true}), "angle"),
        (json!({"channel": "flow", "angle": 10, "speed": "fast", "maskValue": 0.1, "follow": false, "material": "steel"}), "speed"),
        (json!({"channel": "flow", "angle": 10, "speed": 1e100, "maskValue": 0.3, "enabled": false}), "speed"),
        (json!({"channel": "flow", "angle": [90], "speed": 0.5, "maskValue": 0.3, "solo": true}), "angle"),
        (json!({"channel": "flow", "angle": 10, "speed": 0.3, "follow": "yes", "maskValue": 0.2, "material": "steel"}), "follow"),
        (json!({"channel": "roughness", "value": 0.7, "follow": true, "maskValue": 0.2, "solo": true}), "follow"),
        (json!({"channel": "flow", "value": [1, 1, 0], "maskValue": 0, "enabled": false, "follow": false, "material": "steel"}), "value"),
        (json!({"channel": "flow", "value": "sideways", "maskValue": 0, "solo": true, "follow": false}), "value"),
        (json!({"channel": "flow", "value": [0.5, 1e40, 0], "maskValue": 0, "solo": true}), "value"),
        (json!({"channel": "nowhere", "angle": 10, "maskValue": 0, "material": "steel", "solo": true}), "channel"),
        (json!({"channel": "flow", "angle": 10, "speed": 0.4, "maskValue": 0, "material": "no such material", "solo": true}), "material"),
        (json!({"angle": 30, "maskValue": 0, "material": "steel"}), "angle"),
        (json!({"follow": false, "maskValue": 0, "material": "steel"}), "follow"),
    ]
}

#[test]
fn a_refused_material_request_leaves_the_whole_tool_as_it_was() {
    let mut session = flow_session();
    material(&mut session, json!({"channel": "flow", "angle": 135.0, "speed": 0.4, "follow": false}));
    material(&mut session, json!({"material": "copper"}));
    material(&mut session, json!({"maskValue": 0.75}));
    let before = (tool_state(&mut session), inspected_tool(&mut session), session.revision, session.history_labels());
    assert_eq!(before.0["material"], json!("copper"));

    for (request, bad_parameter) in rejected_material_requests() {
        let error = execute(&mut session, "tool.material", &request).unwrap_err().to_string();
        assert!(error.contains(bad_parameter), "{request}: {error}");

        assert_eq!(tool_state(&mut session), before.0, "{request}");
        assert_eq!(session.revision, before.2, "{request}");
    }
    assert_eq!(inspected_tool(&mut session), before.1);
    assert_eq!(session.history_labels(), before.3);
}

#[test]
fn a_refused_fill_request_leaves_the_project_as_it_was() {
    let mut session = flow_session();
    let fill = flow_fill(&mut session);
    let still = execute(&mut session, "layer.add.fill", &json!({"color": [0.5, 0.5, 0.5]})).unwrap()["layer"].as_u64().unwrap();
    execute(&mut session, "layer.fill.set", &json!({"layer": still, "channel": "flow", "enabled": true})).unwrap();
    session.update();
    let before = (session.doc.project.clone(), session.history_labels(), execute(&mut session, "project.inspect", &Value::Null).unwrap());

    let requests = [
        (json!({"layer": fill, "channel": "flow", "angle": "north", "speed": 1.0, "enabled": true, "solo": true}), "angle"),
        (json!({"layer": fill, "channel": "flow", "angle": 90.0, "speed": [1.0], "solo": true}), "speed"),
        (json!({"layer": fill, "channel": "flow", "angle": 90.0, "speed": 1e300, "enabled": false}), "speed"),
        (json!({"layer": fill, "channel": "flow", "value": [1, 1, 0], "solo": true}), "value"),
        (json!({"layer": fill, "channel": "flow", "color": [0, 0, 0], "enabled": false}), "color"),
        (json!({"layer": fill, "channel": "flow", "value": {"x": 1}, "solo": true}), "value"),
        (json!({"layer": fill, "channel": "roughness", "angle": 30.0, "value": 0.2, "solo": true}), "angle"),
        (json!({"layer": still, "channel": "flow", "angle": 200.0, "solo": true}), "speed"),
    ];
    for (request, bad_parameter) in requests {
        let error = execute(&mut session, "layer.fill.set", &request).unwrap_err().to_string();
        assert!(error.contains(bad_parameter), "{request}: {error}");

        assert_eq!(session.doc.project, before.0, "{request}");
        assert_eq!(session.history_labels(), before.1, "{request}");
    }
    assert_eq!(execute(&mut session, "project.inspect", &Value::Null).unwrap(), before.2);
}

#[test]
fn a_malformed_parameter_is_refused_with_its_name_and_what_it_takes() {
    let mut session = flow_session();
    let fill = flow_fill(&mut session);

    let malformed = [
        ("angle", json!("north"), "degrees"),
        ("angle", json!([1, 2]), "degrees"),
        ("angle", json!(true), "degrees"),
        ("angle", json!({"degrees": 90}), "degrees"),
        ("angle", json!(1e100), "degrees"),
        ("speed", json!("fast"), "0 to 1"),
        ("speed", json!([0.5]), "0 to 1"),
        ("speed", json!(false), "0 to 1"),
        ("speed", json!(-1e300), "0 to 1"),
        ("value", json!("east"), "[r, g, b]"),
        ("value", json!([1, "a", 0]), "[r, g, b]"),
        ("value", json!([0.5, 1e40, 0]), "[r, g, b]"),
        ("value", json!({"x": 1, "y": 0}), "[r, g, b]"),
    ];
    for (key, given, accepted) in &malformed {
        // A good sibling must not get through beside the bad one
        let sibling = if *key == "angle" { "speed" } else { "angle" };
        let mut request = json!({"channel": "flow", sibling: 0.25});
        if *key == "value" {
            request = json!({"channel": "flow"});
        }
        request[*key] = given.clone();

        let brush_before = brush_flow(&session);
        let error = execute(&mut session, "tool.material", &request).unwrap_err().to_string();
        assert!(error.contains(&format!("`{key}`")) && error.contains(accepted), "brush {request}: {error}");
        assert_eq!(brush_flow(&session), brush_before, "{request}");

        request["layer"] = json!(fill);
        let error = execute(&mut session, "layer.fill.set", &request).unwrap_err().to_string();
        assert!(error.contains(&format!("`{key}`")) && error.contains(accepted), "fill {request}: {error}");
        assert_eq!(fill_flow(&session, fill), flow_from_polar(0.0, 0.5), "{request}");
    }

    // `follow` is the brush's alone
    for given in [json!("yes"), json!(1), json!([true])] {
        let error = execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": given, "speed": 0.1})).unwrap_err().to_string();
        assert!(error.contains("`follow`") && error.contains("true or false"), "{error}");
    }

    // Absent, or null (how JSON carries NaN and infinity), keeps the current value
    let before = brush_flow(&session);
    material(&mut session, json!({"channel": "flow", "angle": null, "speed": null, "follow": null, "value": null}));
    material(&mut session, json!({"channel": "flow"}));
    assert_eq!(brush_flow(&session), before);
}

#[test]
fn a_packed_value_faster_than_the_maximum_speed_is_refused() {
    let mut session = flow_session();
    let fill = flow_fill(&mut session);
    let brush_before = brush_flow(&session);

    for request in
        [json!({"channel": "flow", "value": [1, 1, 0]}), json!({"channel": "flow", "color": [0, 0, 0]}), json!({"channel": "flow", "value": [1, 0.52, 0]})]
    {
        let error = execute(&mut session, "tool.material", &request).unwrap_err().to_string();
        assert!(error.contains("maximum speed is 1"), "{error}");
        let mut on_fill = request.clone();
        on_fill["layer"] = json!(fill);
        let error = execute(&mut session, "layer.fill.set", &on_fill).unwrap_err().to_string();
        assert!(error.contains("maximum speed is 1"), "{error}");
    }

    // The message shows the length the value decodes to
    let error = execute(&mut session, "tool.material", &json!({"channel": "flow", "value": [1, 1, 0]})).unwrap_err().to_string();
    assert!(error.contains("1.414"), "{error}");
    assert_eq!(brush_flow(&session), brush_before);
    assert_eq!(fill_flow(&session, fill), flow_from_polar(0.0, 0.5));

    // A full-speed diagonal as the canvas stores it is not too fast, and blue is emptied as before
    let diagonal = flow_from_polar(45.0, 1.0);
    material(&mut session, json!({"channel": "flow", "value": [diagonal[0], diagonal[1], 0.8]}));
    assert_eq!(brush_flow(&session), diagonal);
    execute(&mut session, "layer.fill.set", &json!({"layer": fill, "channel": "flow", "value": [diagonal[0], diagonal[1], 0.8]})).unwrap();
    assert_eq!(fill_flow(&session, fill), diagonal);
}

#[test]
fn the_brush_keeps_its_direction_through_a_speed_of_zero() {
    let mut session = flow_session();
    material(&mut session, json!({"channel": "flow", "angle": 90.0, "speed": 1.0}));

    let stopped = material(&mut session, json!({"channel": "flow", "speed": 0.0}));
    assert_eq!(brush_flow(&session), FLOW_STILL);
    assert_eq!(stopped["flow"], json!({"follow": false, "angle": 90.0, "speed": 0.0, "value": FLOW_STILL}));

    let resumed = material(&mut session, json!({"channel": "flow", "speed": 1.0}));
    assert_eq!(brush_flow(&session), flow_from_polar(90.0, 1.0), "the speed comes back the way it went");
    assert_eq!(resumed["flow"]["angle"], json!(90.0));
}

#[test]
fn a_direction_given_at_rest_keeps_the_speed_and_is_remembered() {
    let mut session = flow_session();
    material(&mut session, json!({"channel": "flow", "speed": 0.0}));

    let turned = material(&mut session, json!({"channel": "flow", "angle": 200.0}));
    assert_eq!(brush_flow(&session), FLOW_STILL, "one alone keeps the other: the speed stays 0");
    assert_eq!(turned["flow"]["angle"], json!(200.0));
    assert_eq!(tool_state(&mut session)["flow"], json!({"follow": false, "angle": 200.0, "speed": 0.0, "value": FLOW_STILL}));

    material(&mut session, json!({"channel": "flow", "speed": 0.5}));
    assert_eq!(brush_flow(&session), flow_from_polar(200.0, 0.5));
}

#[test]
fn a_direction_keeps_the_brush_material() {
    let mut session = flow_session();
    material(&mut session, json!({"material": "steel"}));

    for request in [
        json!({"channel": "flow", "angle": 45.0, "speed": 0.5}),
        json!({"channel": "flow", "speed": 0.8}),
        json!({"channel": "flow", "follow": true}),
        json!({"channel": "flow", "value": flow_from_polar(10.0, 1.0)}),
    ] {
        let reply = material(&mut session, request.clone());
        assert_eq!(reply["material"], json!("steel"), "{request}");
    }
    assert_eq!(brush_flow(&session), flow_from_polar(10.0, 1.0));

    // A value of the material's own channels still makes the brush a plain one
    assert_eq!(material(&mut session, json!({"channel": "roughness", "value": 0.2}))["material"], Value::Null);
}

#[test]
fn the_tool_state_and_inspect_report_the_material_and_the_flow_settings() {
    let mut session = flow_session();
    const SPEED: f32 = 0.6;
    material(&mut session, json!({"channel": "flow", "angle": 0.0, "speed": SPEED, "solo": true}));
    let east = (tool_state(&mut session), inspected_tool(&mut session));

    material(&mut session, json!({"channel": "flow", "angle": 90.0}));
    let north = (tool_state(&mut session), inspected_tool(&mut session));

    // Both reports tell the two brushes apart, by the direction and by the packed value
    for (report, angle) in [(&east.0, 0.0f32), (&east.1, 0.0), (&north.0, 90.0), (&north.1, 90.0)] {
        assert_eq!(report["flow"], json!({"follow": false, "angle": angle, "speed": SPEED, "value": flow_from_polar(angle, SPEED)}), "{report}");
    }
    assert_ne!(east.1, north.1, "inspect shows the edit");

    // Inspect lists the channels the brush paints with their values, and only those
    assert_eq!(north.1["material"]["enabled"], json!(["flow"]));
    assert_eq!(north.1["material"]["values"], json!({"flow": flow_from_polar(90.0, SPEED)}));

    material(&mut session, json!({"channel": "roughness", "value": 0.25}));
    material(&mut session, json!({"channel": "flow", "follow": true}));
    material(&mut session, json!({"maskValue": 0.5}));
    let report = inspected_tool(&mut session);
    assert_eq!(report["material"]["enabled"], json!(["roughness", "flow"]));
    assert_eq!(report["material"]["values"]["roughness"], json!([0.25, 0.25, 0.25]));
    assert_eq!(report["material"]["maskValue"], json!(0.5));
    assert_eq!(report["material"]["material"], Value::Null);
    assert_eq!(report["flow"]["follow"], json!(true));

    material(&mut session, json!({"material": "steel"}));
    assert_eq!(inspected_tool(&mut session)["material"]["material"], json!("steel"));
}

#[test]
fn a_histogram_of_the_flow_map_is_refused_by_name() {
    let mut session = flow_session();
    let effect = execute(&mut session, "effect.add.levels", &json!({"target": "content"})).unwrap()["effect"].as_u64().unwrap();

    // Levels pass the flow map through, so it has no levels to show
    let error = execute(&mut session, "effect.histogram", &json!({"effect": effect, "channel": "flow"})).unwrap_err().to_string();
    assert!(error.contains("flow") && error.contains("levels"), "{error}");

    let roughness = execute(&mut session, "effect.histogram", &json!({"effect": effect, "channel": "roughness"})).unwrap();
    assert_eq!(roughness["channel"], json!("roughness"));

    // A layer that paints the flow map alone has nothing levels could remap either
    let layer = session.selected_layer().unwrap();
    session.doc.project.layer_mut(layer).unwrap().channels.retain(|channel, _| *channel == Channel::Flow);
    let error = execute(&mut session, "effect.histogram", &json!({"effect": effect})).unwrap_err().to_string();
    assert!(error.contains("flow"), "{error}");
}
