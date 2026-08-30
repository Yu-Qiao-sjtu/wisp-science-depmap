# De novo miniprotein binder design campaign: prompt release bundle

Contents
--------
prompts/
  multi_target_binder_design_prompt.md   the 14-target, 48 h / $50k campaign prompt (system prompt for every agent)
  single_target/<TARGET>.md              16 single-target, 24 h / $10k campaign prompts (shared template): the 15 targets of the
                                         main campaign plus GDF-8-latent.md, the latent (pro-form) GDF-8 follow-up campaign
  kickoff/multi_target_kickoff.md        the user message that starts a multi-target campaign (T0 message)
  kickoff/single_target_kickoff.md       the user message that starts a single-target campaign
  Figure 1.jpg, Figure 2.jpg             the two figures the prompts reference (LCP definition and LCP results);
                                         attach them to the kickoff message together with the prompt markdown
corpus/                                  the External Resource Corpus the prompts point to (317 files, 1.16 GB):
                                         cited web pages, ProteinBase collections, method/model papers, prompt figures,
                                         and the companion co-folding benchmark paper. Start at corpus/CORPUS_README.txt
                                         and corpus/master_source_index.csv.
REDISTRIBUTION_NOTES.csv                 one row per corpus source with provenance URL and licence class
MANIFEST.sha256                          sha256 + size of every file in this bundle

How the pieces fit
------------------
1. Choose a prompt (multi-target, or one single-target file). Load it as the agent's system context AND attach the
   same markdown file plus Figure 1.jpg / Figure 2.jpg to the first user message.
2. Send the matching kickoff message (prompts/kickoff/*.md) as that first user message. T0 = its timestamp.
3. Make the corpus/ folder readable by the agent (e.g. upload it to a Drive/S3/volume location of your choosing).
   The prompts refer to it as "the External Resource Corpus distributed with this prompt (folder corpus/)"; every
   "corpus folder" reference (e.g. "07 Prompt-Cited Papers", "06 Prompt Figures", "02 ProteinBase") is a sub-folder
   of corpus/.
4. Fill the operator placeholders before launch: <campaign-slack-channel> / <SLACK_CHANNEL_ID>,
   <Campaign Deliverables Folder> / <DRIVE_DELIVERABLES_FOLDER_URL>, and the Emergency-contact line
   (<operator Slack handle>, <operator email>, <operator phone>). Nothing else needs editing. Note that the prompts assume the Claude Science agent harness (host.delegate,
   host.compute, submit_gate, wait_for_notification, ...), a Slack channel and a Google-Drive deliverables folder supplied
   by the operator, and an operator-funded Modal account; a public reader reproducing the campaign must supply equivalents.

What was changed for release (relative to the prompts as run)
--------------------------------------------------------------
- Personal contact details, the Slack channel name/ID and the Google-Drive folder names/IDs were replaced by the
  placeholders above; the named human operator was replaced by "the campaign operator".
- References to the private Drive copy of the corpus now point at the bundled corpus/ folder.
- A few organization-specific references with no bearing on the task were removed or replaced by their public
  equivalents (the agent harness is referred to as "Claude Science" throughout). No task, method, budget, scoring or
  deliverable specification was altered.
- An internal bookkeeping column was dropped from the corpus index; CORPUS_README.txt was rewritten
  for the offline layout; the benchmark paper files were given a shorter neutral filename; the figure rows of the
  index were renumbered to match the two figures actually referenced (Figure 1 = LCP definition, Figure 2 = LCP
  results).

The latent GDF-8 prompt
-----------------------
prompts/single_target/GDF-8-latent.md is the prompt of the follow-up campaign against latent GDF-8 (pro-myostatin,
the unprocessed precursor). It is the GDF-8 prompt with the target redefined; the four differences are:
1. Task line: "human latent Myostatin / GDF-8 (the pro-form precursor, promyostatin)" and "human latent GDF-11" as
   the counter-target.
2. Target-definition paragraph: latent GDF-8 (UniProt O14793) described as the unprocessed precursor assembled as a
   C2-symmetric 2:2 complex (two disulfide-linked growth-factor chains, each non-covalently caged by its N-terminal
   prodomain); latent GDF-11 (UniProt O95390) as the same 2:2 precursor architecture.
3. One sentence appended to the competition reference: the ProteinBase competition page describes mature GDF-8 and
   references PDB 3HH2, whereas the assay antigen is the latent precursor.
4. Scoring item 7, oligomer clause: "latent GDF-8 and latent GDF-11 are C2-symmetric 2:2 assemblies" (was
   "GDF-8 and GDF-11 are homodimers").
Everything else (methods, budgets, scoring, deliverables, kickoff message, corpus) is identical to GDF-8.md, and the
same release substitutions were applied.

Notes
-----
- The prompts reference tooling of the Claude Science agent harness (host.delegate, host.compute, submit_gate,
  wait_for_notification, Modal sandboxes tagged claude-science-project, ...). They are reproduced as run.
- corpus/ files are convenience copies of third-party sources retrieved June 2026; source_url in the index is
  authoritative and REDISTRIBUTION_NOTES.csv gives the licence class of each. Large PDFs are split into page-range
  parts that open natively; four large supplementary archives are byte-split (see corpus/05 Oversized Files/
  Archives/_CHUNKS_MANIFEST.json for reassembly).
