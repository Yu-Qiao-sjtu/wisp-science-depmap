# Publication wide-layout QA

Actual Leptos frontend rendered in Chromium with the mocked Tauri bridge and
synthetic evidence, not a packaged desktop or real research dataset.

- `evidence-2560.png`: 2560 × 1440, Chinese, three evidence columns.
- `new-publication-1920.png`: 1920 × 1080, Chinese, guidance beside a bounded form.

`ui-tests/tests/publication-layout.spec.ts` also checks 3840 × 2160, dark mode,
1280/900/600-pixel widths, creation, and source/binding navigation. Set
`WISP_PUBLICATION_SHOTS` to an output directory to regenerate review images.
