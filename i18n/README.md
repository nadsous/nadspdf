# Interface translations

NadsPDF's interface catalogs, embedded at build time by `crates/ui-egui/build.rs` (no environment
variable needed). The interface code keeps English strings as lookup keys; these files map them to
each language.

## Where they come from

- `locales/<code>/common.tsv` and `locales/<code>/pdfcraft.tsv` are copied from
  [storytold/craft-i18n](https://github.com/storytold/craft-i18n) at the commit in
  `../craft-i18n.rev` (575cfdef63227b14daf173b8cc87928bfdfa325e), with `PdfCraft` replaced by
  `NadsPDF` in both columns, as the interface strings are. Licence: MIT OR Apache-2.0
  (`LICENSE-MIT`, `LICENSE-APACHE`, from craft-i18n).
- `locales/<code>/nadspdf.tsv` holds NadsPDF's own rows: strings only NadsPDF has (vaults,
  bubbles, extensions, conversion, compression…) and wordings NadsPDF chose over the shared
  catalog. `build.rs` merges it over `pdfcraft.tsv`: a row here replaces the row with the same
  context and source text.

The format is craft-i18n's `docs/catalog-format.md`: `context <TAB> English source <TAB>
translation`, with `@id` (command-id keyed) and `@plural` (CLDR forms, `|`-separated) rows.

## Refreshing from craft-i18n

1. Check out craft-i18n at the new commit and write that SHA to `../craft-i18n.rev`.
2. Copy `locales/<code>/common.tsv` and `pdfcraft.tsv` for every locale that has a
   `pdfcraft.tsv` over the files here.
3. Replace `PdfCraft` with `NadsPDF` in the copied files (`PDFCRAFT_` variables, `pdfcraft-cli`
   and lowercase `pdfcraft` stay).
4. Leave `nadspdf.tsv` alone. Rows it no longer needs (craft-i18n now has the same translation)
   can be deleted.

To build against a craft-i18n checkout instead of this copy, set `CRAFT_I18N_DIR` to it.

## Adding a NadsPDF string

Add the row to `locales/<code>/nadspdf.tsv` for each language you translate; a missing row shows
in English. Translate from the plain meaning of the English text, without consulting another
product's translations.
