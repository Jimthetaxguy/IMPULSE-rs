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

/// Props for one block.
#[derive(Props, Clone, PartialEq)]
pub struct BlockViewProps {
    pub block: Block,
}

/// Render a single `Block` as a `<li>` card with status icon, command,
/// optional exit code, and output.
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
}

/// Render a list of blocks as an ordered list.
///
/// The list is ordered (`<ol>`) so block IDs naturally form a numbered
/// log readable to the user. Reverse the iteration order in CSS via
/// `flex-direction: column-reverse` if you want newest-first.
#[component]
pub fn BlockListView(props: BlockListViewProps) -> Element {
    let blocks = props.blocks;
    rsx! {
        ol {
            class: "impulse-block-list",
            for block in blocks.iter() {
                BlockView {
                    key: "{block.id}",
                    block: block.clone(),
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
        let mut vdom = VirtualDom::new_with_props(BlockView, BlockViewProps { block });
        vdom.rebuild_in_place();
        dioxus_ssr::render(&vdom)
    }

    fn render_list_to_string(blocks: Vec<Block>) -> String {
        let mut vdom = VirtualDom::new_with_props(BlockListView, BlockListViewProps { blocks });
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
        };
        assert_eq!(props.block.id, 7);
        assert_eq!(props.block.command.as_deref(), Some("pwd"));
    }
}
