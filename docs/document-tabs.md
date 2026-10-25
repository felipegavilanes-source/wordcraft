# Optional document tabs (desktop)

WordCraft can keep multiple documents inside one window, with a tab strip beneath the title bar.
This feature is **off by default**.

## Turn it on

Go to **File → Options → General → Interface** and enable **Show document tabs (like a web browser)**.
The preference is stored in `ui.json` as `documentTabs`.

When enabled:
- **New (+)** creates a separate blank editing session; File → New / Open opens new tabs.
- **Select a tab** to return to that document, with its own selection, undo history and scroll state.
- **Close (×)** closes only that document; changes are confirmed with the existing Save / Don't Save / Cancel prompt.
- **Drag a tab** onto another tab to change its position.
- **Ctrl+Tab / Ctrl+Shift+Tab** (Cmd on macOS) cycles documents.
- **File → Options → General → Restore saved document tabs** remembers the paths of open, saved files across application launches. An unsaved document without a path cannot be reopened by path.
- Opening an already open path focuses its existing document rather than opening the file twice.

The feature is desktop-only for now: browser document downloads may complete asynchronously and require per-tab save tickets to avoid losing confirmation when switching.

## Safety

Each document has a separate `Session`, so the undo history, caret, and document model do not overwrite each other. Crash recovery runs for inactive tabs as well as for the foreground session. Closing the window does not silently discard dirty tabs. Disabling tabs while more than one document is open is refused so no tab becomes inaccessible.

**Limit:** 24 tabs per window, to bound memory use.

## Quality checks for a future upstream pull request

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p wordcraft-ui-egui document_tabs`
- [ ] `cargo xtask ci`
- [ ] Desktop smoke test: open two DOCX files, edit them, switch, undo independently, save one, close the other.
- [ ] Check restoration when a file was moved or deleted, and when a file is password-protected.
- [ ] Run on Windows, macOS and Linux.

This feature branch should be rebased onto the upstream current main before proposing a pull request.
