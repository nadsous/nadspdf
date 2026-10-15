# pdfcraft-extensions

Out-of-process extensions for NadsPDF: a folder with a `manifest.json` and a program, started by
the desktop app and spoken to in newline-delimited JSON-RPC 2.0 over stdin/stdout. The contract
for extension authors is [`docs/extensions.md`](../../docs/extensions.md).

- **Layer:** L7, headless. Depends on no UI toolkit and no other workspace crate; `ui-egui` uses it
  (`extensions_ui.rs`) and answers the requests against the engine.
- **Modules:** `folders` (where extensions and approvals live; the app adds portable mode), `manifest` (parse and check: unknown fields refused, program inside the folder,
  exact web origins), `discovery` (the installed folders, broken ones with their reason, SHA-256
  fingerprint of manifest + program), `approvals` (what the user turned on, bound to the
  fingerprint; a corrupt file approves nothing), `protocol` (messages, params, limits; a bad line
  becomes an error value, never a panic) and `host` (native only: start, bounded line reading,
  stop by closing stdin then ending the program).
- **Opt-in:** nothing here starts a program by itself. The app starts an extension only when
  `Approvals::is_approved` holds for it.

```sh
cargo test -p pdfcraft-extensions -j 2
```
