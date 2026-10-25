//! Optional multi-document tab strip. Each tab owns its entire editing session and viewport.
//! No DOCX bytes are rewritten when switching documents.
use std::path::{Path, PathBuf};

use egui::{RichText, Sense, vec2};
use serde_json::{Value, json};
use wordcraft_engine::Session;

use crate::{WordApp, canvas, content_controls, panes, theme::Tokens};

pub(crate) const MAX_TABS: usize = 24;

/// An inactive document, including the undo stack, caret/scroll and per-document UI state.
pub(crate) struct DocumentTab {
    session: Session,
    canvas: canvas::CanvasState,
    controls: content_controls::ControlUi,
    comment_drafts: panes::CommentDrafts,
    autosave_path: Option<PathBuf>,
    no_document: Option<u64>,
    last_autosave: f64,
}

fn title(session: &Session) -> String {
    session.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())
        .or_else(|| session.file_name.clone())
        .or_else(|| (!session.doc.core.title.is_empty()).then(|| session.doc.core.title.clone()))
        .unwrap_or_else(|| "Document".into())
}

impl WordApp {
    /// The web save picker has one active-document callback; until it can track per-tab tickets,
    /// tabs are desktop-only. Never hide a saved tab when the preference is switched off.
    pub(crate) fn tabs_visible(&self) -> bool {
        !cfg!(target_arch = "wasm32") && (self.ui.document_tabs || !self.document_tabs.is_empty())
    }

    pub(crate) fn tab_count(&self) -> usize {
        self.document_tabs.len() + 1
    }

    pub(crate) fn tab_info(&self, index: usize) -> Option<(String, bool)> {
        if index >= self.tab_count() {
            return None;
        }
        if index == self.active_document_tab {
            return Some((title(&self.session), self.session.dirty));
        }
        let slot = if index < self.active_document_tab { index } else { index - 1 };
        self.document_tabs.get(slot).map(|t| (title(&t.session), t.session.dirty))
    }

    /// A successful Open/New starts a separate session instead of replacing an edited document.
    pub(crate) fn opens_document_tab(&self, id: &str, params: &Value) -> bool {
        self.ui.document_tabs && !cfg!(target_arch = "wasm32") && self.has_document()
            && (id == "file.new" || (id == "file.open" && params.get("path").is_some()))
    }

    pub(crate) fn open_document_tab(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if self.dialog.is_some() || self.awaiting_save.is_some() || self.save_unconfirmed() {
            return Err("Finish the current dialog or save before opening another tab".into());
        }
        // Opening the same saved file again focuses its existing session, including unsaved edits.
        if id == "file.open"
            && let Some(path) = params.get("path").and_then(Value::as_str)
        {
            let wanted = Path::new(path);
            for i in 0..self.tab_count() {
                let existing = if i == self.active_document_tab {
                    self.session.path.as_deref()
                } else {
                    let slot = if i < self.active_document_tab { i } else { i - 1 };
                    self.document_tabs.get(slot).and_then(|t| t.session.path.as_deref())
                };
                if existing == Some(wanted) {
                    self.switch_document_tab(i)?;
                    return Ok(json!({"activatedTab": i}));
                }
            }
        }

        if self.tab_count() >= MAX_TABS {
            return Err(format!("Too many open documents (limit {MAX_TABS}); close a tab before opening another"));
        }
        let mut fresh = Session::new(self.session.blank_document());
        fresh.system_region = self.session.system_region.clone();
        fresh.prefs = self.session.prefs.clone();
        fresh.keymap = self.session.keymap.clone();
        fresh.ribbon = self.session.ribbon.clone();
        fresh.author = self.session.author.clone();
        fresh.initials = self.session.initials.clone();
        fresh.user_dict_path = self.session.user_dict_path.clone();
        fresh.autosave = self.session.autosave;
        fresh.view.dark_mode = self.session.view.dark_mode;
        fresh.view.zoom = self.session.view.zoom;
        fresh.set_editing_language(crate::i18n::Lang::from_pref(&self.ui.language).editing_lang());

        let previous = self.active_document_tab;
        let tab = DocumentTab {
            session: std::mem::replace(&mut self.session, fresh),
            canvas: std::mem::take(&mut self.canvas),
            controls: std::mem::take(&mut self.controls),
            comment_drafts: std::mem::take(&mut self.comment_drafts),
            autosave_path: self.autosave_path.take(),
            no_document: self.no_document.take(),
            last_autosave: self.last_autosave,
        };
        self.document_tabs.insert(previous, tab);
        self.active_document_tab = previous + 1;
        self.last_autosave = 0.0;
        // Only this internal call replaces the empty staging session.
        self.opening_document_tab = true;
        let result = self.execute(id, params);
        self.opening_document_tab = false;
        if result.is_err() {
            // An invalid path or unsupported document must not create an empty tab.
            if previous < self.document_tabs.len() {
                let old = self.document_tabs.remove(previous);
                self.session = old.session;
                self.canvas = old.canvas;
                self.controls = old.controls;
                self.comment_drafts = old.comment_drafts;
                self.autosave_path = old.autosave_path;
                self.no_document = old.no_document;
                self.last_autosave = old.last_autosave;
            }
            self.active_document_tab = previous;
        } else {
            self.ui.backstage = false;
            self.ui.tab = crate::DEFAULT_TAB.into();
            self.comment_fonts_for = None;
            if let Some(ctx) = &self.ctx { ctx.request_repaint(); }
        }
        result
    }

    pub(crate) fn switch_document_tab(&mut self, index: usize) -> Result<Value, String> {
        if index >= self.tab_count() {
            return Err("Document tab does not exist".into());
        }
        if index == self.active_document_tab {
            return Ok(json!({"activeTab": index}));
        }
        if self.dialog.is_some() || self.colors.is_some() || self.awaiting_save.is_some() || self.save_unconfirmed() || self.printing.busy() {
            return Err("Finish the current dialog, save or print before switching documents".into());
        }
        let slot = if index < self.active_document_tab { index } else { index - 1 };
        let Some(other) = self.document_tabs.get_mut(slot) else { return Err("Document tab does not exist".into()) };
        std::mem::swap(&mut self.session, &mut other.session);
        std::mem::swap(&mut self.canvas, &mut other.canvas);
        std::mem::swap(&mut self.controls, &mut other.controls);
        std::mem::swap(&mut self.comment_drafts, &mut other.comment_drafts);
        std::mem::swap(&mut self.autosave_path, &mut other.autosave_path);
        std::mem::swap(&mut self.no_document, &mut other.no_document);
        std::mem::swap(&mut self.last_autosave, &mut other.last_autosave);
        self.active_document_tab = index;
        self.ui.backstage = false;
        self.ui.tab = crate::DEFAULT_TAB.into();
        self.comment_fonts_for = None;
        self.status_msg = None;
        if let Some(ctx) = &self.ctx { ctx.request_repaint(); }
        Ok(json!({"activeTab": index}))
    }

    pub(crate) fn close_active_document_tab(&mut self) -> Result<Value, String> {
        if self.document_tabs.is_empty() {
            return Err("No other document tab is open".into());
        }
        let previous = self.active_document_tab;
        let next = if previous > 0 { previous - 1 } else { 1 };
        self.switch_document_tab(next)?;
        let discarded = if previous < next { previous } else { previous - 1 };
        self.document_tabs.remove(discarded);
        if previous < next {
            self.active_document_tab = self.active_document_tab.saturating_sub(1);
        }
        Ok(json!({"closed": true, "activeTab": self.active_document_tab}))
    }

    /// Chrome-like drag reorder while retaining the live editor in the active slot.
    pub(crate) fn reorder_document_tab(&mut self, from: usize, to: usize) {
        if from == to || from >= self.tab_count() || to >= self.tab_count() {
            return;
        }
        if from == self.active_document_tab {
            self.active_document_tab = to;
            return;
        }
        let mut active = self.active_document_tab;
        let old_slot = if from < active { from } else { from - 1 };
        let item = self.document_tabs.remove(old_slot);
        if from < active { active -= 1; }
        let new_slot = if to <= active { to } else { to - 1 };
        self.document_tabs.insert(new_slot, item);
        if to <= active { active += 1; }
        self.active_document_tab = active;
    }

    pub(crate) fn other_unsaved_tab(&self) -> Option<usize> {
        (0..self.tab_count()).filter(|&i| i != self.active_document_tab).find(|&i| {
            let slot = if i < self.active_document_tab { i } else { i - 1 };
            self.document_tabs.get(slot).is_some_and(|t| t.session.dirty)
        })
    }
}

pub(crate) fn bar(app: &mut WordApp, ui: &mut egui::Ui) {
    if !app.tabs_visible() || app.ui.backstage || app.session.view.focus_mode {
        return;
    }
    let t = Tokens::get(ui.ctx());
    enum Action { Switch(usize), Close(usize), New, Move(usize, usize) }
    let mut action = None;
    egui::TopBottomPanel::top("document_tabs")
        .exact_height(36.0)
        .frame(egui::Frame::NONE.fill(t.title_bar).inner_margin(egui::Margin::symmetric(8, 3)))
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                egui::ScrollArea::horizontal().id_salt("tab_strip_scroll").auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for index in 0..app.tab_count() {
                            let Some((name, dirty)) = app.tab_info(index) else { continue };
                            let selected = app.active_document_tab == index;
                            let title = if name.chars().count() > 22 {
                                format!("{}…", name.chars().take(21).collect::<String>())
                            } else { name.clone() };
                            let label = if dirty { format!("● {title}") } else { title };
                            let button = egui::Button::new(RichText::new(label).color(if selected { t.text } else { t.text_dim }))
                                .selected(selected).min_size(vec2(126.0, 27.0)).sense(Sense::click_and_drag());
                            let response = ui.add(button).on_hover_text(name);
                            if response.drag_started() { app.drag_document_tab = Some(index); }
                            if response.hovered() && ui.input(|i| i.pointer.any_released())
                                && let Some(from) = app.drag_document_tab
                                && from != index
                            {
                                action = Some(Action::Move(from, index));
                            } else if response.clicked() {
                                action = Some(Action::Switch(index));
                            }
                            if ui.add(egui::Button::new(RichText::new("×").color(t.text_dim)).small())
                                .on_hover_text("Close document")
                                .clicked()
                            {
                                action = Some(Action::Close(index));
                            }
                        }
                    });
                });
                if ui.add(egui::Button::new(RichText::new("+").color(t.accent)))
                    .on_hover_text("New document")
                    .clicked()
                {
                    action = Some(Action::New);
                }
            });
        });
    if ui.input(|i| i.pointer.any_released()) { app.drag_document_tab = None; }
    match action {
        Some(Action::Switch(i)) => { if let Err(e) = app.switch_document_tab(i) { app.status(e); } }
        Some(Action::Close(i)) => {
            if let Err(e) = app.switch_document_tab(i).and_then(|_| app.run("file.close", json!({}))) {
                app.status(e);
            }
        }
        Some(Action::Move(from, to)) => app.reorder_document_tab(from, to),
        Some(Action::New) => { if let Err(e) = app.run("file.new", json!({})) { app.status(e); } }
        None => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use wordcraft_doc::{Document, StoryRef};

    fn app() -> WordApp {
        WordApp::new(Session::new(Document::new()), crate::Services::default())
    }

    fn body(app: &WordApp) -> String {
        app.session.doc.plain_text(StoryRef::Body)
    }

    #[test]
    fn tabs_are_opt_in_and_the_preference_survives_restart() {
        let mut app = app();
        assert!(!app.ui.document_tabs);
        assert!(app.run("ui.documentTabs", json!({"value": "yes"})).is_err());
        app.run("ui.documentTabs", json!({"value": true})).unwrap();
        assert!(app.tabs_visible());
        let json = serde_json::to_vec(&app.prefs()).unwrap();
        let mut restored = WordApp::new(Session::new(Document::new()), crate::Services::default());
        restored.apply_prefs(crate::UiState::from_json(&json).unwrap());
        assert!(restored.ui.document_tabs);
    }

    #[test]
    fn new_tab_preserves_unsaved_text_undo_and_cursor() {
        let mut app = app();
        app.run("ui.documentTabs", json!({"value": true})).unwrap();
        app.run("text.insert", json!({"text": "Original"})).unwrap();
        let original_cursor = app.session.sel.clone();
        app.run("file.new", json!({})).unwrap();
        assert_eq!(app.tab_count(), 2);
        assert_eq!(body(&app).trim(), "");
        app.run("text.insert", json!({"text": "Second"})).unwrap();
        app.switch_document_tab(0).unwrap();
        assert_eq!(body(&app).trim(), "Original");
        assert_eq!(app.session.sel, original_cursor);
        assert!(app.session.dirty);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(body(&app).trim(), "");
        app.switch_document_tab(1).unwrap();
        assert_eq!(body(&app).trim(), "Second");
    }

    #[test]
    fn closing_one_tab_prompts_before_discard_and_keeps_the_other() {
        let mut app = app();
        app.run("ui.documentTabs", json!({"value": true})).unwrap();
        app.run("text.insert", json!({"text": "Keep"})).unwrap();
        app.run("file.new", json!({})).unwrap();
        app.run("text.insert", json!({"text": "Discard"})).unwrap();
        assert_eq!(app.run("file.close", json!({})).unwrap(), json!({"pending": "saveChanges"}));
        assert_eq!(app.tab_count(), 2);
        app.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        assert_eq!(app.tab_count(), 2);
        app.run("file.close", json!({})).unwrap();
        app.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        assert_eq!(app.tab_count(), 1);
        assert_eq!(body(&app).trim(), "Keep");
        assert!(app.run("ui.documentTabs", json!({"value": false})).is_ok());
    }

    #[test]
    fn cannot_hide_multiple_open_documents_or_reorder_out_of_range() {
        let mut app = app();
        app.run("ui.documentTabs", json!({"value": true})).unwrap();
        app.run("file.new", json!({})).unwrap();
        assert!(app.run("ui.documentTabs", json!({"value": false})).is_err());
        assert_eq!(app.tab_count(), 2);
        app.reorder_document_tab(200, 0);
        assert_eq!(app.tab_count(), 2);
        app.reorder_document_tab(1, 0);
        assert_eq!(app.active_document_tab, 0);
    }

    #[test]
    fn failed_open_does_not_replace_or_duplicate_any_document() {
        let mut app = app();
        app.run("ui.documentTabs", json!({"value": true})).unwrap();
        app.run("text.insert", json!({"text": "Never lose this"})).unwrap();
        assert!(app.run("file.open", json!({"path": "/no-such-wordcraft-tab-file-987321.docx"})).is_err());
        assert_eq!(app.tab_count(), 1);
        assert_eq!(body(&app).trim(), "Never lose this");
    }
}
