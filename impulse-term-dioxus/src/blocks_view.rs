//! Dioxus components that render `Block`s and `BlockStore`s.
//!
//! # Render shape
//!
//! ```html
//! <ol class="impulse-block-list">
//!   <li class="impulse-block impulse-block-finished" data-block-id="0" data-state="finished">
//!     <header class="impulse-block-header">
//!       <span class="impulse-block-status">✓</span>
//!       <code class="impulse-block-command">ls -la</code>
//!       <span class="impulse-block-exit">exit 0</span>
//!     </header>
//!     <pre class="impulse-block-output">file1.txt
//! file2.txt
//! </pre>
//!   </li>
//!   ...
//! </ol>
//! ```
//!
//! # Memoization
//!
//! Each `BlockView` is a `#[component]` whose props are `Block` (which
//! derives `PartialEq`). When the parent passes the same block (same id +
//! same content + same state), Dioxus skips re-rendering it. The
//! append-only nature of `BlockStore` means typically only the last
//! block changes between ticks — perfect fit for prop-equality memoization.
//!
//! # Status (L171)
//!
//! Visual structure only. L172 adds the toolbar (copy / rerun /
//! send-to-AI buttons). L173 adds sticky-scroll for the running block.
//! L174 adds gutter decorations.

use dioxus::prelude::*;
use impulse_term_core::{Block, BlockState};

/// Actions the toolbar can request. The component itself is side-effect
/// free — it emits these events and the consumer (e.g. `impulse-supervisor`)
/// wires them to clipboard / PTY / AI integrations.
///
/// Keeping these as data + callback (rather than baking clipboard/PTY
/// access into the component) keeps `blocks_view.rs` testable without a
/// webview, and keeps consumer apps free to choose their own clipboard
/// strategy (web `navigator.clipboard` vs `arboard` native, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockAction {
    /// User clicked "copy command" — wants `block.command` on the clipboard.
    CopyCommand { block_id: u64, text: String },
    /// User clicked "copy output" — wants `block.output` on the clipboard.
    CopyOutput { block_id: u64, text: String },
    /// User clicked "rerun" — wants `block.command + "\n"` written to the PTY.
    Rerun { block_id: u64, command: String },
    /// User clicked "ask AI" — wants the block handed to the agent for
    /// insight extraction. Carries `Block::full_text()` so the consumer
    /// doesn't need to look up the block.
    AskAi { block_id: u64, full_text: String },
}

/// Props for one block.
#[derive(Props, Clone, PartialEq)]
pub struct BlockViewProps {
    pub block: Block,
    /// Optional toolbar action handler. If `None`, the toolbar is hidden.
    /// If `Some`, four buttons appear in the header (copy command, copy
    /// output, rerun, ask AI) emitting `BlockAction` values.
    #[props(default)]
    pub on_action: Option<EventHandler<BlockAction>>,
}

/// Render a single `Block` as a `<li>` card with status icon, command,
/// optional exit code, output, and (if `on_action` is provided) a toolbar
/// of action buttons.
#[component]
pub fn BlockView(props: BlockViewProps) -> Element {
    let block = props.block;
    let state_class = state_class(block.state);
    let state_data_attr = state_data_attr(block.state);
    let icon = block.state.status_icon();
    let exit_label = exit_label(block.state);
    let id = block.id;
    let command_text = block.command.clone().unwrap_or_default();
    let output_text = block.output.clone();
    let full_text = block.full_text();
    let has_toolbar = props.on_action.is_some();
    let on_action = props.on_action;

    let copy_command = command_text.clone();
    let copy_output = output_text.clone();
    let rerun_command = command_text.clone();
    let ask_full = full_text;

    rsx! {
        li {
            class: "impulse-block {state_class}",
            "data-block-id": "{id}",
            "data-state": "{state_data_attr}",
            header {
                class: "impulse-block-header",
                span { class: "impulse-block-status", "{icon}" }
                code { class: "impulse-block-command", "{command_text}" }
                if !exit_label.is_empty() {
                    span { class: "impulse-block-exit", "{exit_label}" }
                }
                if has_toolbar {
                    nav {
                        class: "impulse-block-toolbar",
                        "data-block-id": "{id}",
                        button {
                            class: "impulse-block-action impulse-block-action-copy-command",
                            "data-action": "copy-command",
                            r#type: "button",
                            title: "Copy command",
                            disabled: copy_command.is_empty(),
                            onclick: move |_| {
                                if let Some(h) = on_action {
                                    h.call(BlockAction::CopyCommand {
                                        block_id: id,
                                        text: copy_command.clone(),
                                    });
                                }
                            },
                            "⧉ cmd"
                        }
                        button {
                            class: "impulse-block-action impulse-block-action-copy-output",
                            "data-action": "copy-output",
                            r#type: "button",
                            title: "Copy output",
                            disabled: copy_output.is_empty(),
                            onclick: move |_| {
                                if let Some(h) = on_action {
                                    h.call(BlockAction::CopyOutput {
                                        block_id: id,
                                        text: copy_output.clone(),
                                    });
                                }
                            },
                            "⧉ out"
                        }
                        button {
                            class: "impulse-block-action impulse-block-action-rerun",
                            "data-action": "rerun",
                            r#type: "button",
                            title: "Rerun command",
                            disabled: rerun_command.is_empty(),
                            onclick: move |_| {
                                if let Some(h) = on_action {
                                    h.call(BlockAction::Rerun {
                                        block_id: id,
                                        command: rerun_command.clone(),
                                    });
                                }
                            },
                            "↻ rerun"
                        }
                        button {
                            class: "impulse-block-action impulse-block-action-ask-ai",
                            "data-action": "ask-ai",
                            r#type: "button",
                            title: "Send block to AI",
                            onclick: move |_| {
                                if let Some(h) = on_action {
                                    h.call(BlockAction::AskAi {
                                        block_id: id,
                                        full_text: ask_full.clone(),
                                    });
                                }
                            },
                            "✨ ai"
                        }
                    }
                }
            }
            if !output_text.is_empty() {
                pre {
                    class: "impulse-block-output",
                    "{output_text}"
                }
            }
        }
    }
}

/// Props for the block list.
#[derive(Props, Clone, PartialEq)]
pub struct BlockListViewProps {
    pub blocks: Vec<Block>,
    /// Forwarded to every child `BlockView`. If `None`, no toolbars render.
    #[props(default)]
    pub on_action: Option<EventHandler<BlockAction>>,
}

/// Render a list of blocks as an ordered list.
///
/// The list is ordered (`<ol>`) so block IDs naturally form a numbered
/// log readable to the user. Reverse the iteration order in CSS via
/// `flex-direction: column-reverse` if you want newest-first.
#[component]
pub fn BlockListView(props: BlockListViewProps) -> Element {
    let blocks = props.blocks;
    let on_action = props.on_action;
    rsx! {
        ol {
            class: "impulse-block-list",
            for block in blocks.iter() {
                BlockView {
                    key: "{block.id}",
                    block: block.clone(),
                    on_action,
                }
            }
        }
    }
}

/// CSS state class for one block. Renderers can style each state
/// distinctly (e.g. red border for non-zero exit).
fn state_class(state: BlockState) -> &'static str {
    match state {
        BlockState::PromptShown => "impulse-block-prompt",
        BlockState::AwaitingCommand => "impulse-block-input",
        BlockState::Streaming => "impulse-block-streaming",
        BlockState::Finished(Some(0)) => "impulse-block-finished impulse-block-success",
        BlockState::Finished(Some(_)) => "impulse-block-finished impulse-block-failure",
        BlockState::Finished(None) => "impulse-block-finished impulse-block-unknown",
    }
}

/// data-state attribute value (machine-readable). Distinct from the CSS
/// class because data-state describes the lifecycle phase only, not the
/// success/failure of finished blocks.
fn state_data_attr(state: BlockState) -> &'static str {
    match state {
        BlockState::PromptShown => "prompt",
        BlockState::AwaitingCommand => "input",
        BlockState::Streaming => "streaming",
        BlockState::Finished(_) => "finished",
    }
}

/// Display label for the exit code, empty for non-finished states.
fn exit_label(state: BlockState) -> String {
    match state {
        BlockState::Finished(Some(0)) => "exit 0".into(),
        BlockState::Finished(Some(n)) => format!("exit {n}"),
        BlockState::Finished(None) => "exit ?".into(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_term_core::BlockStore;

    fn render_block_to_string(block: Block) -> String {
        let mut vdom = VirtualDom::new_with_props(
            BlockView,
            BlockViewProps {
                block,
                on_action: None,
            },
        );
        vdom.rebuild_in_place();
        dioxus_ssr::render(&vdom)
    }

    fn render_block_with_toolbar_to_string(block: Block) -> String {
        // EventHandler::new must be constructed inside a Dioxus runtime
        // context, so we wrap BlockView in a tiny component that creates
        // the handler from inside its own scope.
        #[derive(Props, Clone, PartialEq)]
        struct Wrapper {
            block: Block,
        }
        #[component]
        fn ToolbarWrapper(props: Wrapper) -> Element {
            let on_action = EventHandler::new(|_action: BlockAction| {});
            rsx! {
                BlockView {
                    block: props.block.clone(),
                    on_action,
                }
            }
        }

        let mut vdom = VirtualDom::new_with_props(ToolbarWrapper, Wrapper { block });
        vdom.rebuild_in_place();
        dioxus_ssr::render(&vdom)
    }

    fn render_list_to_string(blocks: Vec<Block>) -> String {
        let mut vdom = VirtualDom::new_with_props(
            BlockListView,
            BlockListViewProps {
                blocks,
                on_action: None,
            },
        );
        vdom.rebuild_in_place();
        dioxus_ssr::render(&vdom)
    }

    fn make_finished(id: u64, command: &str, output: &str, exit: i32) -> Block {
        let mut store = BlockStore::new();
        for _ in 0..=id {
            store.open_prompt();
        }
        store.set_command(command);
        store.open_output();
        store.append_output(output);
        store.close_with_exit(Some(exit));
        store.current().unwrap().clone()
    }

    #[test]
    fn test_state_class_for_each_state() {
        assert_eq!(state_class(BlockState::PromptShown), "impulse-block-prompt");
        assert_eq!(
            state_class(BlockState::Finished(Some(0))),
            "impulse-block-finished impulse-block-success"
        );
        assert_eq!(
            state_class(BlockState::Finished(Some(1))),
            "impulse-block-finished impulse-block-failure"
        );
        assert_eq!(
            state_class(BlockState::Finished(None)),
            "impulse-block-finished impulse-block-unknown"
        );
    }

    #[test]
    fn test_state_data_attr_collapses_finished() {
        // All Finished(*) variants share data-state="finished" — the
        // success/failure split is in the CSS class, not the attribute.
        assert_eq!(state_data_attr(BlockState::Finished(Some(0))), "finished");
        assert_eq!(state_data_attr(BlockState::Finished(Some(1))), "finished");
        assert_eq!(state_data_attr(BlockState::Finished(None)), "finished");
        assert_eq!(state_data_attr(BlockState::Streaming), "streaming");
    }

    #[test]
    fn test_exit_label_finished_zero() {
        assert_eq!(exit_label(BlockState::Finished(Some(0))), "exit 0");
        assert_eq!(exit_label(BlockState::Finished(Some(127))), "exit 127");
        assert_eq!(exit_label(BlockState::Finished(None)), "exit ?");
        assert_eq!(exit_label(BlockState::PromptShown), "");
        assert_eq!(exit_label(BlockState::Streaming), "");
    }

    #[test]
    fn test_render_finished_block_includes_command_and_exit() {
        let block = make_finished(0, "ls -la", "total 4\nfile1.txt\n", 0);
        let html = render_block_to_string(block);
        assert!(html.contains("ls -la"), "expected command in HTML: {html}");
        assert!(
            html.contains("exit 0"),
            "expected exit label in HTML: {html}"
        );
        assert!(html.contains("✓"), "expected ✓ status icon: {html}");
        assert!(
            html.contains("data-block-id=\"0\""),
            "expected block id attr: {html}"
        );
        assert!(
            html.contains("data-state=\"finished\""),
            "expected finished data-state: {html}"
        );
    }

    #[test]
    fn test_render_failed_block_uses_failure_class_and_x() {
        let block = make_finished(5, "false", "", 1);
        let html = render_block_to_string(block);
        assert!(
            html.contains("impulse-block-failure"),
            "expected failure class: {html}"
        );
        assert!(html.contains("✗"), "expected ✗ icon: {html}");
        assert!(html.contains("exit 1"), "expected exit 1 label: {html}");
    }

    #[test]
    fn test_render_streaming_block_omits_exit_label() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_command("sleep 5");
        store.open_output();
        store.append_output("waiting...\n");
        let block = store.current().unwrap().clone();
        let html = render_block_to_string(block);
        assert!(
            !html.contains("exit"),
            "streaming block should not show exit label: {html}"
        );
        assert!(html.contains("⟳"), "expected streaming icon: {html}");
        assert!(
            html.contains("data-state=\"streaming\""),
            "expected streaming state: {html}"
        );
    }

    #[test]
    fn test_render_block_without_output_omits_pre() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_command("noop");
        store.close_with_exit(Some(0));
        let block = store.current().unwrap().clone();
        let html = render_block_to_string(block);
        assert!(
            !html.contains("impulse-block-output"),
            "expected no <pre> for empty output: {html}"
        );
    }

    #[test]
    fn test_render_list_emits_one_li_per_block() {
        let blocks = vec![
            make_finished(0, "echo a", "a\n", 0),
            make_finished(1, "echo b", "b\n", 0),
            make_finished(2, "echo c", "c\n", 0),
        ];
        let html = render_list_to_string(blocks);
        let li_count = html.matches("<li").count();
        assert_eq!(li_count, 3, "expected 3 <li>, got {li_count}: {html}");
        assert!(html.contains("impulse-block-list"));
    }

    #[test]
    fn test_render_list_block_ids_in_data_attribute() {
        let blocks = vec![
            make_finished(0, "first", "", 0),
            make_finished(1, "second", "", 0),
            make_finished(2, "third", "", 0),
        ];
        let html = render_list_to_string(blocks);
        assert!(html.contains("data-block-id=\"0\""));
        assert!(html.contains("data-block-id=\"1\""));
        assert!(html.contains("data-block-id=\"2\""));
        // Commands appear in order.
        let pos_first = html.find("first").expect("first present");
        let pos_second = html.find("second").expect("second present");
        let pos_third = html.find("third").expect("third present");
        assert!(pos_first < pos_second);
        assert!(pos_second < pos_third);
    }

    #[test]
    fn test_render_empty_block_list_emits_empty_ol() {
        let html = render_list_to_string(vec![]);
        assert!(html.contains("impulse-block-list"));
        assert!(!html.contains("<li"));
    }

    #[test]
    fn test_block_view_props_round_trip() {
        let block = make_finished(7, "pwd", "/home\n", 0);
        let props = BlockViewProps {
            block: block.clone(),
            on_action: None,
        };
        assert_eq!(props.block.id, 7);
        assert_eq!(props.block.command.as_deref(), Some("pwd"));
    }

    #[test]
    fn test_no_toolbar_when_on_action_is_none() {
        let block = make_finished(0, "ls", "out\n", 0);
        let html = render_block_to_string(block);
        assert!(
            !html.contains("impulse-block-toolbar"),
            "no toolbar expected when on_action=None: {html}"
        );
        assert!(
            !html.contains("data-action="),
            "no action buttons expected: {html}"
        );
    }

    #[test]
    fn test_toolbar_renders_four_action_buttons() {
        let block = make_finished(3, "ls", "out\n", 0);
        let html = render_block_with_toolbar_to_string(block);
        assert!(
            html.contains("impulse-block-toolbar"),
            "expected toolbar: {html}"
        );
        for action in &["copy-command", "copy-output", "rerun", "ask-ai"] {
            assert!(
                html.contains(&format!("data-action=\"{action}\"")),
                "missing action {action}: {html}"
            );
        }
    }

    #[test]
    fn test_toolbar_disables_copy_output_when_output_empty() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_command("noop");
        store.close_with_exit(Some(0));
        let block = store.current().unwrap().clone();
        let html = render_block_with_toolbar_to_string(block);
        // copy-output button should have disabled attribute
        let copy_out_pos = html
            .find("impulse-block-action-copy-output")
            .expect("copy-output button present");
        let following = &html[copy_out_pos..];
        let close = following.find("</button>").unwrap_or(following.len());
        let button_html = &following[..close];
        assert!(
            button_html.contains("disabled"),
            "copy-output should be disabled when output empty: {button_html}"
        );
    }

    #[test]
    fn test_toolbar_disables_rerun_when_command_empty() {
        // Block with empty command (e.g. a prompt that was abandoned).
        let mut store = BlockStore::new();
        store.open_prompt();
        let block = store.current().unwrap().clone();
        let html = render_block_with_toolbar_to_string(block);
        let rerun_pos = html
            .find("impulse-block-action-rerun")
            .expect("rerun button present");
        let following = &html[rerun_pos..];
        let close = following.find("</button>").unwrap_or(following.len());
        let button_html = &following[..close];
        assert!(
            button_html.contains("disabled"),
            "rerun should be disabled when command empty: {button_html}"
        );
    }

    #[test]
    fn test_toolbar_block_id_attr_present() {
        let block = make_finished(42, "echo hi", "hi\n", 0);
        let html = render_block_with_toolbar_to_string(block);
        assert!(
            html.contains("class=\"impulse-block-toolbar\"")
                || html.contains("class=\"impulse-block-toolbar\" data-block-id"),
            "toolbar should have toolbar class: {html}"
        );
        // The toolbar should carry data-block-id="42"
        let toolbar_pos = html.find("impulse-block-toolbar").expect("toolbar present");
        let following = &html[toolbar_pos..];
        let nav_close = following.find('>').unwrap_or(following.len());
        let nav_open_tag = &following[..nav_close];
        assert!(
            nav_open_tag.contains("data-block-id=\"42\""),
            "toolbar should carry block id 42: {nav_open_tag}"
        );
    }

    #[test]
    fn test_block_action_variants_carry_correct_payload() {
        let copy_cmd = BlockAction::CopyCommand {
            block_id: 1,
            text: "ls".into(),
        };
        let copy_out = BlockAction::CopyOutput {
            block_id: 1,
            text: "result\n".into(),
        };
        let rerun = BlockAction::Rerun {
            block_id: 1,
            command: "ls".into(),
        };
        let ask_ai = BlockAction::AskAi {
            block_id: 1,
            full_text: "$ ls\nresult\n".into(),
        };

        // Pattern-match each to confirm exhaustiveness + payload.
        if let BlockAction::CopyCommand { block_id, text } = copy_cmd {
            assert_eq!(block_id, 1);
            assert_eq!(text, "ls");
        } else {
            panic!("expected CopyCommand");
        }
        if let BlockAction::CopyOutput { text, .. } = copy_out {
            assert_eq!(text, "result\n");
        } else {
            panic!("expected CopyOutput");
        }
        if let BlockAction::Rerun { command, .. } = rerun {
            assert_eq!(command, "ls");
        } else {
            panic!("expected Rerun");
        }
        if let BlockAction::AskAi { full_text, .. } = ask_ai {
            assert_eq!(full_text, "$ ls\nresult\n");
        } else {
            panic!("expected AskAi");
        }
    }
}
