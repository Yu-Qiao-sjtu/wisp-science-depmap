# Structure viewer

The molecular viewer renders structures and MD trajectories in its own window using the vendored open-source Mol\* bundle. The window appears on intent — a click or a chat request — and reuses one Mol\* instance: later loads swap content through events instead of navigating, so the view never flickers.

## Opening a structure

- **Artifact button.** Chat results and workspace artifacts for `pdb`, `ent`, `cif`, `mmcif`, `gro` (and trajectory pairs `dcd`/`xtc`/`trr`/`netcdf`/`nc` with their topology) offer *Open in Artifact Viewer*. A trajectory pair is detected by pairing the trajectory with its sibling topology file.
- **Chat.** In a project conversation, ask the assistant to show a structure. The `present_structure` tool accepts a local structure file or a bare four-character RCSB entry id such as `4KC3`; the id is downloaded into the app cache and shared with the command box below, so neither path downloads twice. An optional PyMOL selection (`chain A`, `organic`, `resi 42`) is applied after the load, and `superpose: true` fits a pose onto the structure already on screen by backbone.
- **Command box.** With the window open, type a bare four-character entry id (`4KC3`) to fetch that RCSB entry. Longer input is interpreted as a PyMOL-style command (`show cartoon, chain A`).
- **Runs.** A run that writes structure files (a docking loop emitting `pose-001.pdb`, …) pushes each stable frame into the open viewer automatically. A file is presented only after its size and modification time hold still, so half-written poses never appear.

The chat can keep steering an open window with `control_viewer` (selections, representations, colors, camera, labels, visibility, distance measurement); this tool never opens the window by itself. In the other direction, the *Explain in chat* button sends what is on screen to the main chat as a question — it never starts a run.

## Recent chips

The toolbar lists the most recent structure files. Each chip is clickable and re-presents that file through the normal reuse path. Trajectories contribute only their topology file to this list; to reopen a trajectory pair with playback, use the artifact button, which knows both files.

## Loading feedback

The status line walks through phases: `loading …` while bytes are read, then `building the 3D view …` while Mol\* parses and builds the scene — the slow part for large mmCIF entries. The window chrome follows the desktop UI language (English or Chinese); Mol\*'s own panels stay in Mol\*'s language.

## Security notes

The viewer webview holds no filesystem scope. Structure and trajectory bytes travel only through path-validated read commands with a size cap, and downloads from files.rcsb.org go into the app cache after the same cap check.
