# GitHub Issue Delivery Plan

## Delivery Rule

Use GitHub issues as the delivery surface.

- If write access to FlexNetOS/nu_plugin is available, open follow-up issues there.
- If write access is blocked, maintain exact issue drafts in
  execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.

## Required Issues

| Issue | Purpose | Depends on |
|---|---|---|
| CDB091 | research landscape and current-state audit | none |
| CDB092 | schema extension | CDB091 |
| CDB093 | language/package inventory | CDB091 |
| CDB094 | raw byte/blob fixtures | CDB091 |
| CDB095 | parser-backed summary prototype | CDB091 |
| CDB096 | Python plan | CDB093 |
| CDB097 | Ruby plan | CDB093 |
| CDB098 | TypeScript/JavaScript plan | CDB093 |
| CDB099 | Go/Shell/Nix/config plan | CDB093 |
| CDB100 | single-binary export crate design | CDB091 |
| CDB101 | generated export crate command surface | CDB100 |
| CDB102 | proof gates and round trip | CDB094, CDB100, CDB101 |
| CDB103 | bounded polyglot views | CDB095 |
| CDB104 | security/no-script/no-credential-leak gates | CDB102, CDB103 |
| CDB105 | readiness and release planning | CDB091 through CDB104 |

## Integration Constraint

Issue 215 was opened in FlexNetOS/flexnetos_runner because direct issue creation
in FlexNetOS/nu_plugin returned 403 Resource not accessible by integration.
Unless that changes, checked-in drafts are the authoritative delivery artifact
for this planning package.
