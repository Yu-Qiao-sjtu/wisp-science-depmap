# Docking fixture provenance

This directory is a frozen, offline-verifiable fixture corpus. It contains no
AutoDock Vina or NumPy wheel, executable, virtual environment, or host path.
The verifier performs no network access and imports neither Vina nor NumPy.

## AutoDock Vina Python API oracle identity

- Release: AutoDock Vina v1.2.7
- Git commit: `8eb40404f4f45608acb3b01427587ac049f27c1f`
- Oracle method: official `vina.Vina.score()` Python API
- NumPy version: 1.26.4
- Tag: <https://github.com/ccsb-scripps/AutoDock-Vina/releases/tag/v1.2.7>
- Source: <https://github.com/ccsb-scripps/AutoDock-Vina/tree/8eb40404f4f45608acb3b01427587ac049f27c1f>
- License: Apache License 2.0; the verbatim upstream license is
  `LICENSE-AutoDock-Vina-Apache-2.0.txt`.

Oracle generation is deliberately separate from offline verification. The
maintenance generator requires all five explicit trust inputs:

1. `--vina-python-path`
2. `--vina-wheel-path` and `--vina-wheel-sha256`
3. `--numpy-wheel-path` and `--numpy-wheel-sha256`

It never downloads or installs a package. Before scoring, it verifies each
wheel archive's bytes, filename, distribution name, and version. It then asks
the supplied interpreter to verify the installed Vina and NumPy distribution
versions, module versions, and PEP 610 `direct_url.json` archive SHA256 and
filename. An environment without equivalent archive-hash installation
provenance is rejected.

The trusted archives used for this snapshot are:

| Package | Wheel filename | Published SHA256 |
| --- | --- | --- |
| Vina 1.2.7 | `vina-1.2.7-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | `a6e46560e50e693fb29883b82be8d9a4d74207b4464cfedf2bd5235192f4eb3d` |
| NumPy 1.26.4 | `numpy-1.26.4-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | `675d61ffbfa78604709862923189bad94014bef562cc35cf61d3a07bba02a7ed` |

Both digests are the published PyPI file digests. Committed oracle JSON records
only wheel filenames, digests, versions, normalized API results, fixture
identities, and relative schema references. It records no Windows, WSL, or
other host absolute path.

## Scoring configuration and score-vector mapping

Every case pins `spacing = 0.375`, `seed = 1`, the method
`vina.Vina.score()`, and oracle version 1.2.7. The micro-case box size is
`[8, 8, 8]`; the coincident-carbon clash uses center `[0, 0, 0]`, carbon
contact uses center `[3.8, 0, 0]`, and the hydrogen-bond case uses center
`[3, 0, 0]`. The 1IEP cases use center
`[15.19, 53.9025, 16.917]` and size `[20, 28, 24]`.

Vina 1.2.7 documents the eight Vina/Vinardo score columns as:

`[total, lig_inter, flex_inter, other_inter, flex_intra, lig_intra, torsions, lig_intra best pose]`

The seven-field compatibility object is derived without inventing terms:

- `total` -> estimated free energy
- `lig_inter + flex_inter` -> final intermolecular energy
- `lig_inter` -> ligand-receptor energy
- `other_inter + flex_intra + lig_intra` -> final total internal energy
- `lig_intra` -> ligand internal energy
- `torsions` -> torsional free energy
- the last column -> unbound system energy

Each oracle JSON preserves all eight raw values and the exact scoring weights.

## Offline upstream-byte identity

For every `official_upstream` fixture that records `source.git_blob`, both the
generator and offline verifier recompute the canonical Git blob object ID as
`sha1(b"blob " + decimal_byte_length + b"\\0" + payload)` and require it to
match the recorded 40-hex OID. Before any Vina API call or oracle write, the
generator performs a unified preflight over every selected case's receptor and
ligand: existence, recorded byte count, SHA256, and—when applicable—official
Git blob identity must all pass. It also repeats the blob gate before managed
hash refresh. A preflight failure therefore produces no oracle writes and
cannot legitimize altered upstream bytes by updating checksums.

## Official 1IEP files

The three official files are verbatim blobs from the pinned Vina commit:

| Local path | Upstream path | Git blob |
| --- | --- | --- |
| `official/autodock-vina-v1.2.7/1iep_ligand.pdbqt` | `example/basic_docking/solution/1iep_ligand.pdbqt` | `e3f73c0e6ed9a5c5ddda3e7df271b2351cfda0f3` |
| `official/autodock-vina-v1.2.7/1iep_receptor.pdbqt` | `example/basic_docking/solution/1iep_receptor.pdbqt` | `aa63f0da2ab4ca2772dc011241e451ded256d2d2` |
| `official/autodock-vina-v1.2.7/1iep_receptorH.pdb` | `example/basic_docking/data/1iep_receptorH.pdb` | `a8b87d87db3973b0ce7dfab45078b40d695f076b` |

The Vina repository example is redistributed under Apache-2.0. The underlying
PDB archive structural data are available under the CC0 1.0 Universal Public
Domain Dedication; redistribution is permitted and attribution is encouraged.
The verbatim CC0 legal code is `LICENSE-PDB-CC0-1.0.txt`. The `.pdb` file is a
parser-only fixture and is not treated as a scoring oracle.

Structure citation: Nagar B, Bornmann W, Pellicena P, Schindler T, Veach DR,
Miller WT, Clarkson B, Kuriyan J. Crystal structures of the kinase domain of
c-Abl in complex with the small molecule inhibitors PD173955 and imatinib
(STI-571). Cancer Research. 2002;62:4236-4243.
<https://doi.org/10.2210/pdb1IEP/pdb>

Vina citations:

- Trott O, Olson AJ. Journal of Computational Chemistry. 2010;31(2):455-461.
  <https://doi.org/10.1002/jcc.21334>
- Eberhardt J, Santos-Martins D, Tillack AF, Forli S. Journal of Chemical
  Information and Modeling. 2021;61(8):3891-3898.
  <https://doi.org/10.1021/acs.jcim.1c00203>
- Quiroga R, Villarreal MA. PLOS ONE. 2016;11(5):e0155183.
  <https://doi.org/10.1371/journal.pone.0155183>

## Project-authored fixtures

Files under `project/` were authored for this repository and are distributed
under `AGPL-3.0-only`, matching the project. They cover nested branch ordering
(including a non-axis body atom in every active branch), malformed unclosed
branches, `H`/`HD` aliases, a coincident C-C clash that locks the positive
soft-curl regime, one-pair carbon contact, and a donor-heavy-atom/`HD`
hydrogen-bond geometry. Their coordinates are structural
test inputs; the generated score vectors are the authenticated Vina API
outputs for those exact bytes and pinned boxes.

`manifest.json` records source, license, redistribution, citations, byte
length, SHA256, parser facts, independent Vina/Vinardo tolerances, scoring
configuration, oracle state, and every generated oracle as a managed fixture.
