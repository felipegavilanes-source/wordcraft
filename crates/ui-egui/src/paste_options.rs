//! A small, Word-style Paste Options button beside the end of the latest paste.
//!
//! It follows the pasted text while scrolling/zooming. Switching styles replays **only**
//! the immediately preceding paste, using the source captured at insertion time.

use egui::{Align2, Order, Pos2, vec2};
use serde_json::{Value, json};
use wordcraft_doc::Pos;

use crate::WordApp;

/// The three text pasting choices Word offers after insertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteMode {
    Source,
    Merge,
    Text,
}

impl PasteMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Merge => "merge",
            Self::Text => "text",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::Source => "edit.paste",
            Self::Merge => "edit.pasteMerge",
            Self::Text => "edit.pasteText",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "merge" => Self::Merge,
            "text" => Self::Text,
            _ => Self::Source,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Source => "Paste",
            Self::Merge => "Merge Formatting",
            Self::Text => "Keep Text Only",
        }
    }
}

/// Last successful paste, captured only when it produced an undoable edit.
/// The document, caret and revision guard against replacing unrelated later edits.
pub struct PostPaste {
    pub params: Value,
    pub mode: PasteMode,
    document: u64,
    caret: Pos,
    revision: u64,
    undo_depth: usize,
}

impl PostPaste {
    /// Remember a paste once the engine finished applying it.
    pub fn capture(app: &WordApp, mut params: Value, mode: PasteMode, undo_before: usize) -> Option<Self> {
        if app.session.undo_depth() <= undo_before {
            return None;
        }
        // An in-app rich fragment sometimes has no system text callback. Capture its plain-text
        // counterpart so Keep Text Only can reapply the same clipboard contents later.
        if params.get("text").is_none()
            && let Some(fragment) = app.session.clipboard.as_ref()
            && let Some(obj) = params.as_object_mut()
        {
            obj.insert("text".into(), Value::String(fragment.plain_text()));
        }
        Some(Self {
            params,
            mode,
            document: app.session.document_id(),
            caret: app.session.sel.focus.clone(),
            revision: app.session.rev(),
            undo_depth: app.session.undo_depth(),
        })
    }

    /// A paste is replaceable only while it remains the most recent, untouched edit.
    pub fn current(&self, app: &WordApp) -> bool {
        self.document == app.session.document_id()
            && self.undo_depth == app.session.undo_depth()
            && self.revision == app.session.rev()
            && self.caret == app.session.sel.focus
    }
}

/// An ordinary paste uses the preference from File > Options > Advanced.
/// Picture pastes bypass this path in WordApp::run so they are unaffected.
pub fn default_command(pref: &str, with_text: bool) -> &'static str {
    if with_text { PasteMode::from_key(pref).command() } else { "edit.paste" }
}

/// Switch the last paste's formatting by undoing it and pasting the same captured source once.
/// An unrelated edit, selection movement or another open document cancels the operation.
pub fn switch(app: &mut WordApp, mode: PasteMode) {
    let Some(saved) = app.paste_options.take() else { return };
    if !saved.current(app) {
        return;
    }
    if mode == saved.mode {
        app.paste_options = Some(saved);
        return;
    }
    // Take the original paste off, restoring its pre-paste selection and caret. Do not read
    // the live system clipboard: it may have changed while the menu was open.
    if app.execute_user("edit.undo", json!({})).is_err() {
        return;
    }
    let before = app.session.undo_depth();
    if let Err(err) = app.execute_user(mode.command(), saved.params.clone()) {
        // The original paste remains available as Redo when a replacement fails.
        let _ = app.execute_user("edit.redo", json!({}));
        app.status(err);
        return;
    }
    app.paste_options = PostPaste::capture(app, saved.params, mode, before);
    app.canvas.want_focus = true;
}

/// Put the button just **below and to the right of the pasted text's final caret**, as Word
/// does—not beside the mouse, the ribbon, or a fixed viewport corner.
fn at_caret(caret: Pos2, height: f32, viewport: egui::Rect) -> Pos2 {
    let x = (caret.x + 8.0).clamp(viewport.min.x + 3.0, (viewport.max.x - 82.0).max(viewport.min.x + 3.0));
    let y = (caret.y + height + 3.0).clamp(viewport.min.y + 3.0, (viewport.max.y - 28.0).max(viewport.min.y + 3.0));
    Pos2::new(x, y)
}

/// Paint the paste button and its three paste modes in a foreground pop-up over the document.
/// Moving the caret, editing again, closing the document or opening another dialog removes it.
pub fn show(app: &mut WordApp, ctx: &egui::Context) {
    let Some(state) = app.paste_options.as_ref() else { return };
    if !state.current(app) || app.ui.backstage || app.dialog.is_some() {
        app.paste_options = None;
        return;
    }
    let Some((caret, height)) = crate::canvas::caret_screen(app) else { return };
    let viewport = app.canvas.viewport.unwrap_or_else(|| ctx.screen_rect());
    if !viewport.contains(caret) {
        return; // Keep its document anchor while scrolled away.
    }
    let at = at_caret(caret, height, viewport.intersect(ctx.screen_rect()));
    egui::Area::new(egui::Id::new("post_paste_options")).order(Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.menu_button("📋 ▾", |ui| {
                ui.label(egui::RichText::new(tl!("Paste as:")).strong());
                ui.horizontal(|ui| {
                    for mode in [PasteMode::Source, PasteMode::Merge, PasteMode::Text] {
                        let selected = app.paste_options.as_ref().is_some_and(|p| p.mode == mode);
                        let resp = ui
                            .add(
                                egui::Button::new(match mode {
                                    PasteMode::Source => "▣",
                                    PasteMode::Merge => "▤",
                                    PasteMode::Text => "A",
                                })
                                .selected(selected)
                                .min_size(vec2(42.0, 32.0)),
                            )
                            .on_hover_text(tl!(mode.label()));
                        if resp.clicked() {
                            switch(app, mode);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button(tl!("Set as Default")).clicked() {
                    app.paste_options = None;
                    let _ = app.run("file.options", json!({"pane": "advanced"}));
                    ui.close();
                }
            });
        });
    });
}

/// Match Word's position: immediately underneath and near the text end, clamped to the page area.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_is_aligned_to_paste_not_a_fixed_corner() {
        let view = egui::Rect::from_min_max(Pos2::new(10.0, 100.0), Pos2::new(500.0, 600.0));
        assert_eq!(at_caret(Pos2::new(200.0, 240.0), 18.0, view), Pos2::new(208.0, 261.0));
        assert_eq!(at_caret(Pos2::new(490.0, 590.0), 18.0, view), Pos2::new(418.0, 572.0));
    }

    /// Switching the pop-up style edits the already inserted text, not a second paste.
    #[test]
    fn a_paste_can_be_reformatted_without_duplicating_text() {
        let mut app = WordApp::new(
            wordcraft_engine::Session::new(wordcraft_doc::Document::from_text("Before ")),
            crate::Services::default(),
        );
        app.run("caret.docEnd", json!({})).unwrap();
        app.run("edit.paste", json!({"text": "after"})).unwrap();
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "Before after");
        assert!(app.paste_options.as_ref().is_some_and(|p| p.current(&app)));
        switch(&mut app, PasteMode::Text);
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "Before after");
        assert!(app.paste_options.as_ref().is_some_and(|p| p.mode == PasteMode::Text));
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "Before ");
        assert!(app.paste_options.is_none());
    }

    /// The contextual menu never undoes typing, navigation or a new paste after the first one.
    #[test]
    fn another_edit_dismisses_paste_options() {
        let mut app = WordApp::new(
            wordcraft_engine::Session::new(wordcraft_doc::Document::new()),
            crate::Services::default(),
        );
        app.run("edit.paste", json!({"text": "pasted"})).unwrap();
        assert!(app.paste_options.is_some());
        app.run("text.insert", json!({"text": "!"})).unwrap();
        assert!(app.paste_options.is_none());
        switch(&mut app, PasteMode::Merge);
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "pasted!");
    }

    /// The Advanced setting applies to the normal Paste shortcut without changing clipboard text.
    #[test]
    fn saved_paste_mode_is_used_for_next_text_paste() {
        let mut app = WordApp::new(
            wordcraft_engine::Session::new(wordcraft_doc::Document::new()),
            crate::Services::default(),
        );
        app.ui.default_paste = "merge".into();
        app.run("edit.paste", json!({"text": "hello"})).unwrap();
        assert!(app.paste_options.as_ref().is_some_and(|p| p.mode == PasteMode::Merge));
        assert_eq!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "hello");
        let restored = crate::UiState::from_json(&serde_json::to_vec(&app.ui).unwrap()).unwrap();
        assert_eq!(restored.default_paste, "merge");
    }

    #[test]
    fn paste_modes_keep_the_existing_engine_commands() {
        assert_eq!(PasteMode::Source.command(), "edit.paste");
        assert_eq!(PasteMode::Merge.command(), "edit.pasteMerge");
        assert_eq!(PasteMode::Text.command(), "edit.pasteText");
        assert_eq!(default_command("merge", true), "edit.pasteMerge");
        assert_eq!(default_command("text", false), "edit.paste");
    }
}
