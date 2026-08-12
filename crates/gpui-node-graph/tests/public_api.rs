use gpui::{AppContext, Focusable, IntoElement};
use gpui_node_graph::{NodeGraph, WorldSceneElement, core::*, world::WorldScene};

#[derive(Clone, Debug, PartialEq)]
struct Kind;
impl PortType for Kind {
    fn compatible(_: &Self, _: &Self) -> bool {
        true
    }
}

#[test]
fn stateless_world_scene_is_a_named_deferred_builder() {
    fn accepts_element(_: impl IntoElement) {}
    accepts_element(WorldSceneElement::new(
        "public-world-scene",
        WorldScene::new(),
        Viewport::default(),
    ));
}

#[gpui::test]
fn graph_state_is_an_explicit_caller_owned_focusable_entity(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_node_graph::init(cx);
        gpui_node_graph::set_node_graph_theme(cx, gpui_node_graph::NodeGraphTheme::dark());
        let graph: GraphState<String, String, String, Kind> = GraphState::default();
        let editor = cx.new(|cx| NodeGraph::new(graph, cx));
        let _: gpui::FocusHandle = editor.focus_handle(cx);
        fn accepts_element(_: impl IntoElement) {}
        accepts_element(editor);
    });
}

#[test]
fn typed_actions_are_public_and_namespaced() {
    use gpui_node_graph::actions::*;
    let _ = (
        OpenCatalog,
        DeleteSelection,
        SelectAll,
        CycleRouting,
        FitView,
        Cancel,
        CopySelection,
        Paste,
        Undo,
        Redo,
        GroupSelection,
        UngroupSelection,
        AlignLeft,
        AlignHorizontalCenter,
        AlignRight,
        AlignTop,
        AlignVerticalCenter,
        AlignBottom,
        DistributeHorizontally,
        DistributeVertically,
        SmartGrid,
    );
}
