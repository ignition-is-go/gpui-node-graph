use std::{fs, path::Path};

#[test]
fn public_ui_api_has_no_pascal_functions_or_stateful_compatibility_layer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            assert!(
                !source.contains("Stateful<"),
                "compatibility state in {}",
                path.display()
            );
            assert!(
                !source.contains("pub type EditorEvent"),
                "event alias in {}",
                path.display()
            );
            assert!(
                !source.contains("pub type NodeMenuStyle"),
                "style alias in {}",
                path.display()
            );
            for line in source.lines().map(str::trim) {
                if let Some(name) = line
                    .strip_prefix("pub fn ")
                    .and_then(|rest| rest.split('(').next())
                {
                    assert!(
                        name.chars().next().is_none_or(char::is_lowercase),
                        "PascalCase public function {name} in {}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn rich_view_and_render_once_builder_are_declared_explicitly() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    assert!(source.contains("> gpui::Focusable\n    for NodeGraph"));
    assert!(source.contains("impl gpui::RenderOnce for WorldSceneElement"));
    assert!(source.contains(
        "impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> Render"
    ));
    assert!(source.contains("gpui::EventEmitter<core::GraphEvent"));
}
