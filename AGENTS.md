# Agent Guidelines

Work in this repository from:

```bash
cd /home/flexnetos/FlexNetOS/src/nu_plugin
```

This repo uses the GitKB workflow copied from `src/meta/.kb`.

Use the profile-owned GitKB binary. (The old `FlexNetOS/usr/bin/git-kb` copy is
quarantined-pack residue slated for refactor — see FlexNetOS/LOCAL_WORKAROUNDS.md,
2026-07-07 owner correction.)

```bash
/home/flexnetos/.nix-profile/bin/git-kb status --json
/home/flexnetos/.nix-profile/bin/git-kb board --json
```

GitKB materializes checked-out documents under `.kb/workspaces/main/` in this
repository. The `.kb/store/`, `.kb/.cache/`, and `.kb/workspaces/` directories
are local ignored state.

Read `.kb/AGENTS.md` for the full GitKB workflow and task/context policy.
