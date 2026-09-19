# Wisp Science base upgrades

`wisp-depmap` records its stable upstream base in
`.github/upstream-base.json`. The daily `Upstream base sync` workflow compares
that marker with the latest published release of `xuzhougeng/wisp-science`.

When a newer stable release exists, the workflow:

1. creates `automation/upstream-vX.Y.Z` from the current `main`;
2. merges the signed upstream release history without rewriting DepMap commits;
3. updates the base marker and opens an upgrade pull request;
4. dispatches the normal test workflow for the upgrade commit;
5. leaves the tested upgrade PR for maintainer review and protected merge.

The workflow never merges an upstream upgrade automatically. Branch protection
and required checks remain authoritative. Merge conflicts are
never guessed or auto-resolved. A conflict creates or updates a blocking Issue
with repository-relative filenames, and no upgrade branch is pushed.

The scheduled channel follows stable GitHub Releases only. Upstream `main` can
contain newer merged pull requests, but those unreleased commits are not
automatically incorporated into the product base. A maintainer can run the
workflow manually from the Actions page when an immediate recheck is needed.

After an automated upgrade, verify that product naming remains `wisp-depmap`,
the DepMap MCP contract and privacy tests pass, and release notes still record
the upstream Wisp Science provenance separately from the product version.
