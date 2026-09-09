**Comparison Target**

- Source visual truth: `docs/design-qa/context-usage-source.png`
- Rendered implementation: `docs/design-qa/context-usage-implementation.png`
- Side-by-side evidence: `docs/design-qa/context-usage-comparison.png`
- Viewport and normalization: source `1516 x 671` px; implementation `1516 x 671` CSS px and pixels at device scale factor `1`; no resizing or density normalization was needed.
- State: light theme, native chat after a `79.9K / 300K` usage event, context-usage panel docked above the composer.

**Findings**

- No actionable P0, P1, or P2 differences remain for hierarchy, palette, or category content.
- Layout correction: the docked panel sits in the composer column (above `.composer-inner`) so its width matches the chat dialog, matching the reference composer-aligned card rather than spanning the full chat workspace or covering the transcript.
- Fonts and typography: the implementation follows Wisp's UI font while matching the source's regular-weight title, muted summary, row hierarchy, tabular values, and no-wrap labels.
- Spacing and layout rhythm: denser row/swatch sizing tracks the reference card sitting directly above the composer.
- Colors and visual tokens: all seven category colors, the unused-context track, neutral foregrounds, border, background, and shadow match the source hierarchy while using Wisp surface tokens where appropriate.
- Copy and content: title, percentage, token total, seven English category labels, and values match the source. Equivalent Simplified Chinese strings are included.
- Accounting: native sessions always expose the seven-category breakdown. Legacy persisted totals without a breakdown attribute the used window to Conversation instead of the ACP-only "Agent-managed context" label. ACP sessions still show a single remote total because the protocol does not report categories.

**Open Questions**

- Dark-theme screenshot baselines can still be added when matching source references are available. Narrow-breakpoint behavior now follows the same dock/float clamps (no special-case drawer).

**Comparison History**

1. Initial comparison found a P2 layout mismatch: the panel was capped at `880px` and its footer-relative anchor let it overlap the composer. The panel was re-anchored to the composer container, expanded across the chat workspace, and placed above the composer.
2. The next comparison found a P2 density mismatch: title, rows, and swatches were visibly smaller than the source. Typography, padding, row height, gaps, and swatch size were increased.
3. User feedback rejected the workspace-wide panel: the reference card matches the composer/dialog width. The panel was re-anchored to `.composer-inner` and density was tightened toward the source card.
4. Native usage rows persisted before categorized accounting were mislabeled as Agent-managed context; they now fall back to Conversation while keeping the seven-row schema.
5. The overlay card covered the latest reply and swallowed the first outside click. It now docks in layout flow, can be dragged into a floating window, and resizes with a corner grip. The invisible backdrop is gone.

**Primary Interactions Tested**

- Native usage moves to the bottom-right meter instead of the top bar.
- The meter opens a seven-row categorized panel with matching bar segments.
- Docked panel width matches `.composer-inner` at the reference viewport and leaves the latest reply visible.
- The first click into the composer input closes a docked panel and places the caret.
- Dragging the header undocks a floating window; typing leaves it open; dock button and header double-click re-dock.
- Escape works immediately after opening and closes only the context panel; the composer and meter remain open. A higher overlay still wins the first Escape.
- Legacy totals-only native usage shows Conversation rather than Agent-managed context.

**Implementation Checklist**

- [x] Match source structure, copy, values, colors, radius, and elevation.
- [x] Keep the panel above the composer at dialog width.
- [x] Dock in layout flow; do not cover the conversation or swallow outside clicks.
- [x] Preserve Wisp's existing navigation and design tokens.
- [x] Verify immediate window-level Escape behavior.
- [x] Avoid mislabeling native totals as Agent-managed context.

**Follow-up Polish**

- P3: refresh same-viewport screenshots after the dialog-width re-anchor when a local capture pass is convenient.
- P3: add visual baselines for dark theme and the narrow mobile breakpoint when corresponding source references exist.

final result: passed

---

# Research journey design QA

Selected target: [approved reference](docs/design-qa/research-journey/reference.png).
Implementation: the existing Leptos desktop app, with the Playwright Tauri bridge providing deterministic research records and real fixture file previews. Backend persistence and scope behavior are covered separately by Rust tests.

## Iteration 1

Compared the selected reference with the rendered 1488 × 1058 light desktop view.

- P2: the inspector column was too narrow. Increased its desktop width to align the main timeline and date column with the reference.
- P2: CSV/report previews showed raw text instead of readable content. Rendered actual CSV rows and bounded report excerpts.
- P2: the progress record repeated the daily heading. The heading now opens that record; the expanded body omits the duplicate.
- P2: narrow viewports put the entire source inspector before the daily history and left a gap beside the collapsed sidebar. Source details now follow the feed; choosing an output scrolls to them. The page follows the existing 56 px rail below 900 px.

## Iteration 2

Compared the source and implementation together in [comparison.png](docs/design-qa/research-journey/comparison.png), and reviewed 800 × 900 and 390 × 844 captures.

- Earlier layout and preview findings resolved. Mobile shows the current daily entry before source details.
- P2: turning the daily headline into a button inherited body-size typography. Added a component-specific heading font rule and a browser assertion for 18 px desktop headings.
- Screenshot normalization: wait for the existing sidebar resize transition to finish before taking the dark desktop capture.

## Final verification

The final combined desktop comparison and the 800 × 900, 390 × 844, and dark-theme captures were inspected after the corrections. Desktop headlines now measure 18 px; the compact rail shows the active icon without overlapping label text. No actionable P0/P1/P2 issues remain.

Evidence: [desktop](docs/design-qa/research-journey/desktop.png), [combined comparison](docs/design-qa/research-journey/comparison.png), [narrow](docs/design-qa/research-journey/narrow.png), [mobile](docs/design-qa/research-journey/mobile.png), [dark](docs/design-qa/research-journey/dark.png).

Validation:
- `WISP_CATALOG_OFFLINE=1 RUST_TEST_THREADS=4 cargo test --workspace`: 1,991 passed. Four threads avoid an existing 30-second run-lease fixture expiring under high parallel load.
- `cargo check --target wasm32-unknown-unknown` in `ui`: passed.
- Both workspace/UI formatting checks and `git diff --check`: passed.
- Full Playwright run: 561 passed, 2 existing conditional skips, 1 app-startup timeout. That unchanged connector test passed in isolation afterward.
- Final targeted pass: 11 passed, covering the 9 journey cases, retained graph interactions, and the connector retry. The final compact-sidebar screenshot check also passed.

The browser captures use mock Tauri records; no real SSH host, GPU, model key, or external data service was used.

Remaining P3 differences are intentional integration choices: the existing sidebar/navigation is retained; factual event counts replace the mock's illustrative totals; artifact filenames and real previews replace decorative thumbnails; registration time and exact version are visible rather than claiming every registered file was created that day.

final result: passed
