# DepMap breast-cancer Workflow acceptance

This pre-package acceptance scenario verifies product orchestration without
contacting a real DepMap server.

## Scenario

User direction: `breast cancer`.

Teacher requirement: after extracting evidence for one resolved gene, propose
research topics and independently assess innovation, feasibility, and clinical
translation. After several user turns select one topic, export an illustrated
report. English output must include figures, captions, Results, and Methods
that can be copied into a manuscript draft.

## Offline acceptance gates

- The topic Workflow starts with a cancer-specific data inventory.
- The inventory node calls `lineage_catalog`; the gene-evidence node separately
  calls `depmap_evidence`, so the same large evidence bundle is not duplicated.
- Its DepMap nodes receive `depmap_read`, whose only tools are `depmap_query`
  and `depmap_evidence`.
- A matching topic-design request creates the Workflow draft before evidence,
  Run-history, shell, Skill-search, or file-write calls. A blocked launch does
  not permit manual reconstruction.
- Candidate topics depend on both bounded DepMap evidence and verified
  literature evidence.
- Innovation, feasibility, and clinical-translation reviews are independent
  nodes and score every candidate topic.
- The ranking report depends on all three reviews and retains dissent,
  limitations, and next questions.
- The selected-topic report is a separate, approval-gated Workflow.
- Report evidence is re-queried rather than copied from model memory.
- Figure generation writes actual evidence-backed files or records an explicit
  omission; decorative substitute figures are forbidden.
- The report writes Results, Methods, figure legends, `report.md`, and
  `report.html` beneath `analysis/depmap-agent/reports/`.
- Every numerical claim must trace to bounded query evidence or a validated
  persisted Run.
- A non-significant top list remains a null result and cannot seed a mechanism,
  drug, pathway, or synthetic-lethal claim. Literature claims include traceable
  paper identifiers.

The Rust tests named
`depmap_topic_template_is_evidence_then_independent_review_then_report`,
`breast_cancer_topic_to_report_acceptance_contract_is_complete`, and
`native_depmap_reader_receives_only_the_bounded_query_tool` enforce these
offline gates. They validate orchestration and security boundaries, not the
scientific content of a live server response.

Exported real trajectories are evaluated separately with `wisp-science
trajectory-eval`; see [DepMap Agent benchmark](depmap-agent-benchmark.md). This
separates deterministic orchestration checks from model-cost, tool-error, and
scientific-review gates.

## Live acceptance after data access is available

1. Connect the research server through Wisp Science when server access is needed.
2. Transfer validated outputs to the local knowledge root, or configure an
   already reachable HTTPS/loopback evidence endpoint. The DepMap Agent itself
   must not create an SSH tunnel.
3. Run `depmap_query(mode=status)` and verify release and health.
4. Use a known gene and breast-cancer lineage for one bounded evidence query.
5. Run the topic Workflow and audit every numerical claim against tool output.
6. Select one topic, run the report Workflow, and inspect every generated
   figure, caption, Results paragraph, Methods paragraph, and evidence link.
