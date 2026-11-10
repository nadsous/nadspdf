# DesignCraft in NadsPDF

The engine of NadsPDF's Mise en page mode: [DesignCraft](https://github.com/storytold/designcraft)
at revision `0078f832a7ab8fee64f091b01fed151a512a5901` (MIT OR Apache-2.0, see `LICENSE-*` and
`NOTICE`), without its interface (`ui-egui`), apps, MCP server, test kit and fonts. Line endings
are LF.

## What NadsPDF changed

- **No print production.** `craft-prepress` (separations, the ink manager, the transparency
  flattener) needs a version of the hayro renderer NadsPDF doesn't use, so it is left out:
  - `crates/engine`: no `separations` module and no separations preview command
    (`cmd/prepress.rs`); two re-exports only it used are gone.
  - `crates/pdf`: no PDF/X-1a (`pdfx1a.rs`, `x1a_tests.rs`); asking for it is refused with a
    message. The ink model the PDF export needs for spot colours (`InkSet`, aliases, inks
    printed as process) is `src/ink_model.rs`, copied from craft-prepress's `ink.rs` without
    the colour conversions.
  - `crates/engine/src/cmd/export/text_tests.rs`: the PDF/X-1a test checks the refusal.
- **Fonts.** None are copied here (AGENTS.md §1.4). `crates/fonts` compiles in NadsPDF's own
  Inter and JetBrains Mono (`assets/fonts/`); Source Sans 3 and Source Serif 4 come from
  craft-fonts' `latin-manifest.txt` when `CRAFT_FONTS_DIR` is set (`build.rs`, `fontdb/db.rs`).
  Without it, text in them falls back to Inter.
- **Name.** Exported PDFs say NadsPDF made them (`Creator`/`Producer`, `pdf/src/export/metadata.rs`
  and `booklet.rs`), and recovery files go to NadsPDF's folder (`engine/src/recovery.rs`): the
  interface never names DesignCraft.
- **Assets.** The renderer's golden test images (`crates/render/tests/golden/`) are left out: they
  were drawn with DesignCraft's own fonts. The sRGB profile in `assets/icc/` has its entry in
  NadsPDF's `ATTRIBUTION.toml`.
- **Workspace.** `Cargo.toml` keeps DesignCraft's dependency versions; its `[patch]` and
  `[profile]` sections are NadsPDF's root ones. `craft-pdf` comes from `../craft-pdf`.

## Updating

Diff the new DesignCraft revision against `0078f832…` for the crates kept here, apply it, and
reapply the changes above (search for `NadsPDF` in the sources).
