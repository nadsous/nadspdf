# Extensions

> **Status:** protocol 1, desktop app only (Windows, macOS, Linux, FreeBSD); the web build has no
> extensions. · **Last updated:** 2026-10-15

An extension adds behaviour to NadsPDF without being part of it: a separate program, in any
language, that NadsPDF starts and talks to over its standard input and output. NadsPDF itself
stays the same for everyone; an extension exists only on the computers it was copied to.

## Safety model

- **Nothing runs by default.** NadsPDF starts an extension only after the user turned it on in
  Preferences ▸ Extensions, which lists what it will be able to do.
- **An approval is bound to the exact program.** It records a SHA-256 fingerprint of the manifest
  and the program. A new program or a manifest asking for something else turns the extension off
  until the user approves it again.
- **An extension gets only what its manifest asks for.** Every request is checked against the
  manifest's permissions; `shell.open_url` opens only the origins the manifest lists.
- **An extension is a program running with the user's rights.** It can do anything the user can,
  including reaching the network; NadsPDF does not sandbox it. The permissions limit what it can
  get *from NadsPDF*. Only install extensions you trust.
- **NadsPDF keeps working without it.** A program that crashes, exits or writes garbage is
  stopped and reported in Preferences; it is never restarted on its own.

## Installing

Copy the extension's folder into the extensions folder, then restart NadsPDF and turn it on in
Preferences ▸ Extensions.

| Platform | Extensions folder |
|---|---|
| Windows | `%LOCALAPPDATA%\NadsPDF\Extensions\` |
| macOS | `~/Library/Application Support/NadsPDF/Extensions/` |
| Linux, FreeBSD | `$XDG_DATA_HOME/pdfcraft/extensions/` (default `~/.local/share/pdfcraft/extensions/`) |
| Portable copy | `Extensions\` in the portable data folder |

The approvals are kept beside the folder, in `extension-approvals.json`. Removing an extension's
folder removes the extension; its approval no longer matches anything.

To see what NadsPDF will find, without starting anything:

```sh
pdfcraft-cli extensions list [--dir DIR]     # every folder, usable or not, and whether it is approved
pdfcraft-cli extensions check <folder>       # one extension: its manifest and fingerprint, or why it can't be used
```

`check` exits with an error when the folder can't be used, so a build or an installer can gate on
it.

## The manifest

`<folder>/manifest.json`:

```json
{
  "protocol": 1,
  "id": "com.example.erp",
  "name": "Example ERP",
  "version": "1.0.0",
  "description": "Recognises job numbers in the documents you open.",
  "executable": "bin/example-erp.exe",
  "permissions": ["document_events", "read_text", "banner", "open_url"],
  "open_url": ["https://erp.example.com"]
}
```

| Field | Rule |
|---|---|
| `protocol` | Must be `1`. |
| `id` | 1–64 lowercase letters, digits, `.`, `-`, `_`; not starting with `.`. Unique among installed extensions. |
| `name`, `version` | Shown in Preferences and on the extension's message bars. |
| `description` | Optional, up to 400 characters. |
| `executable` | Relative path inside the folder; no `..`, root or drive. Symlinks are resolved and must stay inside the folder. |
| `permissions` | Any of the permissions below. |
| `open_url` | Origins `shell.open_url` may open: `https://host[:port]` (`http` only for `localhost` and `127.0.0.1`), no path. Required with, and only with, `open_url`. |

Unknown fields and unknown permissions are refused, so a typo can't silently drop a restriction.

| Permission | Gives |
|---|---|
| `document_events` | `document.opened` and `document.closed` |
| `read_text` | `doc.text` |
| `read_file` | `doc.file`, and the `path` in `document.opened` |
| `banner` | `ui.banner.show`, `ui.banner.hide` |
| `open_url` | `shell.open_url` on the listed origins |

## Messages

One JSON-RPC 2.0 object per line, UTF-8, at most 1 MiB per line. NadsPDF writes to the program's
stdin and reads its stdout; stderr goes to NadsPDF's log. The program's working directory is its
folder. On Windows it starts without a console window.

NadsPDF sends **notifications** (no `id`) and **responses** to the extension's requests. The
extension sends **requests** (with a number or string `id`). Lines that aren't valid requests get
an error response when they carry an `id`, and are otherwise ignored, so stray output (a library
printing to stdout) doesn't stop the extension.

Document ids (`doc`) are numbers, valid while the document is open. Pages are 1-based.

### From NadsPDF

| Notification | Params | When |
|---|---|---|
| `host.started` | `{host, version, protocol}` | First line, always |
| `document.opened` | `{doc, name, pages, path}` | A document opened (also every document already open when the extension starts). `path` is `null` without `read_file` or for an unsaved document |
| `document.closed` | `{doc}` | A document closed; its message bars are gone |
| `banner.action` | `{doc, banner, action}` | The user clicked a message bar button |
| `banner.dismissed` | `{doc, banner}` | The user closed a message bar |

When NadsPDF quits, or the user turns the extension off, its stdin closes: the program should
exit. After half a second it is ended.

### From the extension

| Method | Params | Result |
|---|---|---|
| `doc.text` | `{doc, pages?}` — up to 50 page numbers; the first 10 pages when absent | `{doc, pages: [{page, text}]}`, at most 100,000 characters a page. Read in the background; scanned pages without a text layer come back empty. |
| `doc.file` | `{doc}` | `{doc, name, path, dirty}`; an error when the document isn't saved in a file. `dirty` means the file on disk is older than what is on screen. |
| `ui.banner.show` | `{doc, id, text, actions?: [{id, label, primary?}]}` | `{shown: true}`. Shows a message bar above the document, prefixed with the extension's name. Showing the same `id` again replaces it. At most 2 bars per document per extension, 3 buttons a bar; `id`s are 1–64 letters, digits, `.`, `-`, `_`; `text` is one line of up to 300 characters, a label up to 40. |
| `ui.banner.hide` | `{doc, id}` | `{hidden}` (`false` when it wasn't showing) |
| `shell.open_url` | `{url}` | `{opened: true}`. Opens the system browser. |

Errors use the JSON-RPC codes `-32700` (not JSON), `-32600` (not a request), `-32601` (unknown
method), `-32602` (invalid params, with the reason), plus `-32001` (the manifest lacks the
permission, or the URL isn't on a listed origin) and `-32002` (valid, but it can't be done: the
document isn't open, isn't saved…).

### A session

```text
← {"jsonrpc":"2.0","method":"host.started","params":{"host":"NadsPDF","version":"0.7.4","protocol":1}}
← {"jsonrpc":"2.0","method":"document.opened","params":{"doc":1,"name":"facture.pdf","pages":2,"path":null}}
→ {"jsonrpc":"2.0","id":1,"method":"doc.text","params":{"doc":1}}
← {"jsonrpc":"2.0","id":1,"result":{"doc":1,"pages":[{"page":1,"text":"… BE-2026-0042 …"},{"page":2,"text":"…"}]}}
→ {"jsonrpc":"2.0","id":2,"method":"ui.banner.show","params":{"doc":1,"id":"job","text":"Job BE-2026-0042 found","actions":[{"id":"open","label":"Open the job","primary":true}]}}
← {"jsonrpc":"2.0","id":2,"result":{"shown":true}}
← {"jsonrpc":"2.0","method":"banner.action","params":{"doc":1,"banner":"job","action":"open"}}
→ {"jsonrpc":"2.0","id":3,"method":"shell.open_url","params":{"url":"https://erp.example.com/jobs/42"}}
← {"jsonrpc":"2.0","id":3,"result":{"opened":true}}
```

## Code

- `crates/extensions` (`pdfcraft-extensions`, L7, headless): the manifest, discovery and
  fingerprints, approvals, the message types and the program host.
- `crates/ui-egui/src/extensions_ui.rs`: starting approved extensions, the document events, the
  requests, the message bars and Preferences ▸ Extensions.
- `apps/pdfcraft-cli` (`extensions list`, `extensions check`): the headless view of the same
  folder and approvals.
- Tests: `cargo test -p pdfcraft-extensions` (a real program round trip in `tests/host.rs`),
  `cargo test -p pdfcraft-ui-egui --test extensions` (the app end to end) and
  `cargo test -p pdfcraft-cli --test extensions`.

## Revision history

- 2026-10-15: protocol 1 — manifest, approvals bound to a fingerprint, document events,
  `doc.text`, `doc.file`, message bars, `shell.open_url` on listed origins;
  `pdfcraft-cli extensions list|check`.
