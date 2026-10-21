//! Application shortcut installation and workspace/pane action entry points.

pub use crate::browser::ui::{handle_browser_close, handle_browser_open, restore_browser_tabs};

use crate::app_state::AppState;
use crate::config::ShortcutAction;
use crate::split_engine::FocusDirection;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Install all cmux keyboard shortcuts on the application window.
///
/// Uses PropagationPhase::Capture (parent -> child) so the window controller fires
/// BEFORE Ghostty's per-GLArea EventControllerKey. Without capture phase, Ghostty
/// eats Ctrl+D, Ctrl+N, etc. (per RESEARCH.md Pattern 4 and Anti-patterns).
///
/// Shortcut bindings are driven by ShortcutMap (config-driven, D-06).
pub fn install_shortcuts(
    window: &gtk4::ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    sidebar: &gtk4::Box,
    app: &gtk4::Application,
    shortcut_map: crate::config::ShortcutMap,
) {
    let key_ctrl = gtk4::EventControllerKey::new();
    // CRITICAL: Capture phase -- fires before GLArea key handlers.
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    // Named so `GTK_DEBUG=keybindings` reports which controller handled a key.
    key_ctrl.set_name(Some("cmux-shortcuts"));

    let sidebar_clone = sidebar.clone();
    let app_clone = app.clone();
    let debug_keys = std::env::var_os("CMUX_DEBUG_KEYS").is_some_and(|value| value == "1");
    if debug_keys {
        trace_action_activations(window, "win");
        trace_action_activations(app, "app");
    }

    key_ctrl.connect_key_pressed({
        let state = state.clone();
        let window = window.downgrade();
        move |_controller, keyval, keycode, mods| {
            let action = shortcut_map.lookup(mods, keyval);
            let chord = gtk4::gdk::ModifierType::CONTROL_MASK | gtk4::gdk::ModifierType::ALT_MASK;
            if debug_keys && mods.intersects(chord) {
                eprintln!("{}", key_trace(keyval, keycode, mods, action));
            }
            match action {
                // -- Workspace shortcuts --
                Some(ShortcutAction::NewWorkspace) => {
                    handle_new_workspace(&state, &app_clone);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::CloseWorkspace) => {
                    handle_close_workspace(&state, &app_clone);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::NextWorkspace) => {
                    state.borrow_mut().switch_next();
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::PrevWorkspace) => {
                    state.borrow_mut().switch_prev();
                    gtk4::glib::Propagation::Stop
                }
                Some(
                    action @ (ShortcutAction::MoveWorkspaceUp | ShortcutAction::MoveWorkspaceDown),
                ) => {
                    let (from, to) = {
                        let state = state.borrow();
                        let from = state.active_index;
                        let offset = if action == ShortcutAction::MoveWorkspaceUp {
                            -1
                        } else {
                            1
                        };
                        (from, state.adjacent_workspace_in_group(from, offset))
                    };
                    let changed =
                        to.is_some_and(|to| state.borrow_mut().reorder_workspace(from, to));
                    if changed {
                        crate::sidebar::rebuild_grouped_sidebar(&state);
                    }
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::ToggleWorkspaceGroup) => {
                    let group = {
                        let state = state.borrow();
                        state
                            .active_workspace()
                            .and_then(|workspace| workspace.group_id)
                            .and_then(|id| {
                                state
                                    .workspace_groups
                                    .iter()
                                    .find(|group| group.id == id)
                                    .map(|group| (id, !group.collapsed))
                            })
                    };
                    if let Some((id, collapsed)) = group {
                        let _ = state.borrow_mut().update_workspace_group(
                            id,
                            None,
                            None,
                            Some(collapsed),
                            None,
                        );
                        crate::sidebar::rebuild_grouped_sidebar(&state);
                    }
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::RenameWorkspace) => {
                    let (active_index, sidebar_list) = {
                        let s = state.borrow();
                        let idx = s.active_index;
                        let list = s.sidebar_list.clone();
                        (idx, list)
                    };
                    crate::sidebar::start_inline_rename(&sidebar_list, active_index, state.clone());
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::ToggleSidebar) => {
                    let visible = sidebar_clone.is_visible();
                    sidebar_clone.set_visible(!visible);
                    if let Some(engine) = state.borrow_mut().active_split_engine_mut() {
                        engine.focus_active_surface();
                    }
                    gtk4::glib::Propagation::Stop
                }
                // -- Pane split shortcuts --
                Some(ShortcutAction::SplitRight) => {
                    handle_split(&state, false);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::SplitDown) => {
                    handle_split(&state, true);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::ClosePane) => {
                    handle_close_pane(&state, &app_clone);
                    gtk4::glib::Propagation::Stop
                }
                // -- SSH workspace shortcut --
                Some(ShortcutAction::NewSshWorkspace) => {
                    handle_new_ssh_workspace(&state, &app_clone);
                    gtk4::glib::Propagation::Stop
                }
                // -- Focus direction shortcuts --
                Some(ShortcutAction::FocusLeft) => {
                    handle_focus_direction(&state, FocusDirection::Left);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::FocusRight) => {
                    handle_focus_direction(&state, FocusDirection::Right);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::FocusUp) => {
                    handle_focus_direction(&state, FocusDirection::Up);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::FocusDown) => {
                    handle_focus_direction(&state, FocusDirection::Down);
                    gtk4::glib::Propagation::Stop
                }
                // -- Workspace number shortcuts --
                Some(
                    action @ (ShortcutAction::Workspace1
                    | ShortcutAction::Workspace2
                    | ShortcutAction::Workspace3
                    | ShortcutAction::Workspace4
                    | ShortcutAction::Workspace5
                    | ShortcutAction::Workspace6
                    | ShortcutAction::Workspace7
                    | ShortcutAction::Workspace8
                    | ShortcutAction::Workspace9),
                ) => {
                    let idx = match action {
                        ShortcutAction::Workspace1 => 0,
                        ShortcutAction::Workspace2 => 1,
                        ShortcutAction::Workspace3 => 2,
                        ShortcutAction::Workspace4 => 3,
                        ShortcutAction::Workspace5 => 4,
                        ShortcutAction::Workspace6 => 5,
                        ShortcutAction::Workspace7 => 6,
                        ShortcutAction::Workspace8 => 7,
                        ShortcutAction::Workspace9 => 8,
                        _ => unreachable!(),
                    };
                    state.borrow_mut().switch_to_index(idx);
                    gtk4::glib::Propagation::Stop
                }
                // -- Browser shortcuts --
                Some(ShortcutAction::BrowserOpen) => {
                    handle_browser_open(&state);
                    gtk4::glib::Propagation::Stop
                }
                Some(ShortcutAction::BrowserClose) => {
                    handle_browser_close(&state);
                    gtk4::glib::Propagation::Stop
                }
                None if keyval == gtk4::gdk::Key::comma
                    && mods.intersection(
                        gtk4::gdk::ModifierType::CONTROL_MASK
                            | gtk4::gdk::ModifierType::SHIFT_MASK
                            | gtk4::gdk::ModifierType::ALT_MASK
                            | gtk4::gdk::ModifierType::SUPER_MASK,
                    ) == gtk4::gdk::ModifierType::CONTROL_MASK =>
                {
                    if let Some(window) = window.upgrade() {
                        crate::preferences::show(&window, &state);
                    }
                    gtk4::glib::Propagation::Stop
                }
                None if (keyval.to_lower() == gtk4::gdk::Key::i
                    || keyval.to_lower() == gtk4::gdk::Key::u)
                    && mods.intersection(
                        gtk4::gdk::ModifierType::CONTROL_MASK
                            | gtk4::gdk::ModifierType::SHIFT_MASK
                            | gtk4::gdk::ModifierType::ALT_MASK
                            | gtk4::gdk::ModifierType::SUPER_MASK,
                    ) == (gtk4::gdk::ModifierType::CONTROL_MASK
                        | gtk4::gdk::ModifierType::SHIFT_MASK) =>
                {
                    if keyval.to_lower() == gtk4::gdk::Key::i {
                        if let Some(window) = window.upgrade() {
                            crate::inbox_view::show(&window, &state);
                        }
                    } else if let Err((code, _)) = crate::inbox_actions::handle(
                        &mut state.borrow_mut(),
                        crate::inbox::Action::JumpToUnread,
                    ) {
                        crate::diagnostics::event(format_args!(
                            "notification.navigation outcome={code}"
                        ));
                    }
                    gtk4::glib::Propagation::Stop
                }
                None if keyval.to_lower() == gtk4::gdk::Key::p
                    && mods.intersection(
                        gtk4::gdk::ModifierType::CONTROL_MASK
                            | gtk4::gdk::ModifierType::SHIFT_MASK
                            | gtk4::gdk::ModifierType::ALT_MASK
                            | gtk4::gdk::ModifierType::SUPER_MASK,
                    ) == (gtk4::gdk::ModifierType::CONTROL_MASK
                        | gtk4::gdk::ModifierType::SHIFT_MASK) =>
                {
                    if let Some(window) = window.upgrade() {
                        crate::project_palette::show(&window, &state);
                    }
                    gtk4::glib::Propagation::Stop
                }
                // Everything else passes through to Ghostty.
                _ => gtk4::glib::Propagation::Proceed,
            }
        }
    });

    window.add_controller(key_ctrl);
}

/// Create a new workspace with an initial GLArea pane and add it to AppState + GtkStack.
pub fn handle_new_workspace(state: &Rc<RefCell<AppState>>, app: &gtk4::Application) {
    crate::workspace_dialog::show_workspace_dialog(app, state.clone());
}

/// Show close-workspace confirmation dialog. If confirmed, closes the active workspace.
pub fn handle_close_workspace(state: &Rc<RefCell<AppState>>, app: &gtk4::Application) {
    // Cannot close the last workspace.
    let (workspace_id, workspace_count) = {
        let s = state.borrow();
        (s.active_workspace().map(|ws| ws.id), s.workspaces.len())
    };
    if workspace_count <= 1 {
        return; // No-op: cannot close the last workspace
    }

    let dialog = gtk4::MessageDialog::builder()
        .text("Close Workspace?")
        .secondary_text("All panes in this workspace will be closed. This cannot be undone.")
        .modal(true)
        .build();
    if let Some(window) = app.windows().into_iter().next() {
        dialog.set_transient_for(Some(&window));
    }
    dialog.add_button("Keep Workspace", gtk4::ResponseType::Cancel);
    dialog.add_button("Close Workspace", gtk4::ResponseType::Accept);
    dialog.set_default_response(gtk4::ResponseType::Cancel);
    dialog.connect_response({
        let state = state.clone();
        move |dialog, response| {
            if response == gtk4::ResponseType::Accept {
                let mut s = state.borrow_mut();
                if let Some(index) = s
                    .workspaces
                    .iter()
                    .position(|ws| Some(ws.id) == workspace_id)
                {
                    s.close_workspace(index);
                }
            }
            dialog.close();
        }
    });
    dialog.present();
}

/// Log every activation of the group's simple actions, whether from a menu or an accelerator.
fn trace_action_activations(group: &impl IsA<gtk4::gio::ActionMap>, prefix: &'static str) {
    let group = group.as_ref();
    let names = group
        .dynamic_cast_ref::<gtk4::gio::ActionGroup>()
        .map(|group| group.list_actions())
        .unwrap_or_default();
    for name in names {
        if let Some(action) = group
            .lookup_action(&name)
            .and_downcast::<gtk4::gio::SimpleAction>()
        {
            action
                .connect_activate(move |_, _| eprintln!("cmux: action {prefix}.{name} activated"));
        }
    }
}

/// One `CMUX_DEBUG_KEYS=1` line: what the window's capture controller saw and matched.
/// Only Ctrl or Alt chords are logged, never plain typing. This controller runs before GTK's
/// application accelerators and the terminal, so every key press delivered to the window
/// reaches it. No line at all means the compositor or an
/// input method kept the key; a passed-on key that matches a menu accelerator is followed
/// by a `cmux: action ... activated` line.
pub(crate) fn key_trace(
    keyval: gtk4::gdk::Key,
    keycode: u32,
    mods: gtk4::gdk::ModifierType,
    action: Option<ShortcutAction>,
) -> String {
    use gtk4::glib::translate::IntoGlib;
    let name = keyval
        .name()
        .map_or_else(|| format!("{:#x}", keyval.into_glib()), |n| n.to_string());
    let action = action.map_or_else(
        || "none (passed to the focused widget)".to_owned(),
        |a| format!("{a:?}"),
    );
    format!("cmux: key {name} keycode={keycode} mods={mods:?} -> {action}")
}

/// Split the active pane. `vertical=false` -> split right (Ctrl+D), `vertical=true` -> split down.
/// Publishes a session snapshot after a successful split so keyboard and menu layouts survive
/// restart. Returns the new pane ID, or None when there is no active pane. GTK thread only.
pub fn handle_split(state: &Rc<RefCell<AppState>>, vertical: bool) -> Option<u64> {
    let new_pane_id = {
        let mut s = state.borrow_mut();
        let engine = s.active_split_engine_mut()?;
        // The new GLArea is added to the widget tree and marked active inside SplitEngine.
        if vertical {
            engine.split_down()
        } else {
            engine.split_right()
        }
    };
    if new_pane_id.is_some() {
        state.borrow().trigger_session_save();
    }
    new_pane_id
}

/// Close the active pane (Ctrl+Shift+X). Closing the final pane defers to the
/// close-workspace confirmation; any other close publishes a session snapshot. GTK thread only.
pub fn handle_close_pane(state: &Rc<RefCell<AppState>>, app: &gtk4::Application) {
    let closed = {
        let mut s = state.borrow_mut();
        s.active_split_engine_mut()
            .map(|engine| engine.close_active().is_some())
    };
    match closed {
        Some(true) => state.borrow().trigger_session_save(),
        Some(false) => handle_close_workspace(state, app), // Last pane: close the workspace.
        None => {}
    }
}

/// Close one terminal/browser tab. The final tab follows the pane-close flow.
pub fn handle_close_surface_tab(
    state: &Rc<RefCell<AppState>>,
    app: &gtk4::Application,
    uuid: uuid::Uuid,
) {
    crate::diagnostics::event(format_args!("surface-tab close requested uuid={uuid}"));
    let result = state
        .borrow_mut()
        .active_split_engine_mut()
        .map(|engine| engine.close_surface_and_empty_pane(uuid));
    crate::diagnostics::event(format_args!(
        "surface-tab close result uuid={uuid} result={result:?}"
    ));
    match result {
        Some(crate::split_engine::CloseSurfaceResult::Closed) => {
            state.borrow().trigger_session_save();
        }
        Some(crate::split_engine::CloseSurfaceResult::LastSurfaceInPane) => {
            handle_close_workspace(state, app);
        }
        Some(crate::split_engine::CloseSurfaceResult::NotFound) | None => {}
    }
}

/// Open the SSH connect dialog (Ctrl+Shift+S).
pub fn handle_new_ssh_workspace(state: &Rc<RefCell<AppState>>, app: &gtk4::Application) {
    crate::ssh_dialog::show_ssh_dialog(app, state.clone());
}

/// Move focus to adjacent pane in `direction`.
pub fn handle_focus_direction(state: &Rc<RefCell<AppState>>, direction: FocusDirection) {
    let mut s = state.borrow_mut();
    if let Some(engine) = s.active_split_engine_mut() {
        engine.focus_next_in_direction(direction);
    }
}

/// Synchronize cmux's active-pane model with pointer-driven GTK focus changes.
pub fn handle_focus_pane(state: &Rc<RefCell<AppState>>, pane_id: u64) {
    let Ok(mut app_state) = state.try_borrow_mut() else {
        let state = state.clone();
        glib::idle_add_local_once(move || handle_focus_pane(&state, pane_id));
        return;
    };
    if let Some(engine) = app_state.active_split_engine_mut() {
        if engine.activate_pane(pane_id) {
            crate::diagnostics::event(format_args!("pane activated by pointer pane={pane_id}"));
        }
    }
}

/// Create a sibling terminal tab inside the currently focused pane.
pub fn handle_new_terminal_tab(state: &Rc<RefCell<AppState>>) {
    let created = state
        .borrow_mut()
        .active_split_engine_mut()
        .and_then(|engine| engine.new_terminal_tab())
        .is_some();
    if created {
        state.borrow().trigger_session_save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Count split dividers in a saved layout tree.
    fn saved_splits(node: &crate::split_engine::SplitNodeData) -> usize {
        match node {
            crate::split_engine::SplitNodeData::Split { start, end, .. } => {
                1 + saved_splits(start) + saved_splits(end)
            }
            _ => 0,
        }
    }

    /// Take the latest published snapshot and return its active workspace split count.
    fn published_splits(
        rx: &mut tokio::sync::watch::Receiver<Option<crate::session::Snapshot>>,
    ) -> Option<usize> {
        if !rx.has_changed().unwrap() {
            return None;
        }
        let snapshot = rx.borrow_and_update().clone().expect("snapshot published");
        Some(saved_splits(
            &snapshot.workspaces[snapshot.active_index].layout,
        ))
    }

    /// Keyboard/menu splits and pane closes publish the session snapshot, so restart keeps them.
    #[test]
    #[ignore = "requires GTK display; run in GitHub Actions under Xvfb"]
    fn keyboard_split_and_close_publish_session_snapshot() {
        gtk4::init().unwrap();
        let app = gtk4::Application::new(None::<&str>, gtk4::gio::ApplicationFlags::NON_UNIQUE);
        app.register(None::<&gtk4::gio::Cancellable>).unwrap();
        let stack = gtk4::Stack::new();
        let state = AppState::new(
            stack.clone(),
            gtk4::ListBox::new(),
            std::ptr::null_mut(),
            app.clone(),
        );
        state.borrow_mut().create_workspace();
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        state.borrow_mut().session_tx = Some(tx);
        rx.mark_unchanged();

        assert!(handle_split(&state, false).is_some());
        assert_eq!(published_splits(&mut rx), Some(1));
        assert!(handle_split(&state, true).is_some());
        assert_eq!(published_splits(&mut rx), Some(2));

        handle_close_pane(&state, &app);
        assert_eq!(published_splits(&mut rx), Some(1));
    }
}
