# Floating Paste Options — implementation notes

Related upstream issues: storytold/wordcraft#820 and #1747.

## Interaction contract

- Immediately after a successful user-triggered paste into the document, show a compact Paste Options affordance adjacent to the trailing edge of the inserted range, **not** at the mouse pointer or the top toolbar.
- Anchor to the caret's document-space rectangle after paste and transform into screen coordinates on every frame, including zoom and scrolling. Clamp to the viewport while retaining the association with the pasted range.
- Activate by click, keyboard focus or the advertised shortcut. Offer Keep Source Formatting, Merge Formatting, and Keep Text Only when supported by the clipboard payload; offer Picture only when there is a genuinely usable image.
- Changing an option replaces exactly the most recent pasted span using a single undoable operation; never blindly undo and replay after the user edits. A clipboard snapshot must be captured at paste time so subsequent changes to the OS clipboard do not change the options.
- Dismiss on subsequent document edits, caret relocation outside the pasted range, new paste, undo/redo, document switch, or a dialog opening. Do not steal typing focus. Respect keyboard accessibility and screen readers.
- Do not show for paste into Find, comments, dialog controls, or other non-document fields. Keep behavior identical in desktop and web builds.

## Integration points verified on main (2026-10-21)

- `crates/ui-egui/src/lib.rs`: `WordApp::run` dispatches `edit.paste`; platform clipboard inputs are collected here, including HTML and RTF.
- `crates/ui-egui/src/canvas.rs`: canvas state contains the paste-event and keyboard-handling flags; canvas owns caret/page positioning and contextual overlays.
- `crates/ui-egui/src/keys.rs`: keyboard and egui paste-event normalization; ensure keyboard paste and ribbon/context-menu paste enter the same tracking path.
- `crates/ui-egui/src/system_clipboard.rs`: imported system clipboard formats.
- `crates/engine`: authoritative editing/undo and paste modes.

## Required tests before opening an upstream PR

1. Paste via Ctrl+V and ribbon Paste creates the affordance at the end of the inserted range.
2. Scrolling and zooming preserve the anchor; popup does not drift to pointer coordinates.
3. Switch among three options without duplicating text; undo restores the pre-paste document in one step.
4. Typing after paste invalidates the affordance and cannot be lost when invoking it.
5. Plain text, HTML and RTF payloads; empty clipboard; picture-only input.
6. Paste into a dialog does not display the affordance.
7. egui interactive tests, `cargo fmt --check`, `cargo xtask ci` and wasm checks.

This document records acceptance criteria only; the feature itself is **not yet implemented**.
