# Dirty Repo Fixture

This fixture is documentation-only in the source tree. Later no-mutation tests
should copy this directory to a temporary Git repository, create a tracked file
plus an uncommitted edit there, then prove CodeDB leaves the dirty state intact.

The fixture itself stays clean so this package does not commit intentional dirty
repository state.
