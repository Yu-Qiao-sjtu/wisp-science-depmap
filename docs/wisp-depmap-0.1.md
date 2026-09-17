# wisp-depmap 0.1.0 build identity

`wisp-depmap 0.1.0` is the first product build of this repository. It is based
on the Wisp Science v1.13.0 codebase and adds the DepMap Specialist, catalog
readers, remote MCP bridge, natural-language routing, and indexed 26Q1 evidence
interfaces maintained in this fork.

The desktop bundle uses product name `wisp-depmap`, version `0.1.0`, and bundle
identifier `science.wisp-depmap`. This keeps its application data and installer
identity separate from an existing Wisp Science installation. Existing Wisp
Science model and connection settings are therefore not migrated automatically.
The CLI binary and internal protocol identifiers remain compatible with the
upstream engine where changing them would break scripts, persisted data, or MCP
clients.

Update checks and issue links point to
`Yu-Qiao-sjtu/wisp-science-depmap`. A local unsigned build is suitable for
validation; public distribution still requires the repository's signing and
release workflow.
