# No spec deltas, deliberately

This change moves five facts from between two and nine definitions down to one
each. It changes no requirement, adds no capability, and alters nothing the
system does — which is why this directory is empty rather than unfinished.

`openspec status` reports 3/4 artifacts for that reason. `applyRequires` is
`["tasks"]`, so the change is implementable as it stands.

The one thing here that *could* have been a requirement is the guard in task 6
— a build that refuses a second definition of an existing name. It is not
written as one because every spec in `openspec/specs/` describes what the
archive does for the person using it, and none describes what the build
refuses. Adding the first such spec to get a refactor past a validator would be
inventing a requirement to satisfy a tool, which is the opposite of what the
specs are for.
