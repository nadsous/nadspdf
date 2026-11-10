# nads-extensions

NadsPDF's own extensions, written against protocol 1 like anyone's
([`docs/extensions.md`](../../docs/extensions.md) is the contract and describes what they do):

- `nads-factures`: **Contrôle des factures** (`site.nads.factures`), `src/factures.rs`.
- `nads-echeances`: **Échéances** (`site.nads.echeances`), `src/echeances.rs`.

`src/rpc.rs` is the extension side of the protocol (bounded line reading, `doc.text` for each
opened document, banners), `src/store.rs` the data folder, `src/text.rs` char-safe text helpers.
The checks and the date finder are pure functions, tested on synthetic text.

- **Layer:** exempt (programs NadsPDF starts, not part of it); no workspace dependency.
- **Network:** none. **Permissions:** `document_events`, `read_text`, `banner`.

```sh
cargo test -p nads-extensions -j 2
cargo xtask extensions        # dist/extensions/<id>/, ready for Home ▸ Extensions ▸ Import
```
