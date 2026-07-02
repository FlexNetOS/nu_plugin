# Agent Guidelines

Work in this repository from:

```bash
cd /home/flexnetos/FlexNetOS/src/nu_plugin
```

This repo uses the GitKB workflow copied from `src/meta/.kb`.

Use the workspace-provided GitKB binary explicitly unless your shell has already
prepended `/home/flexnetos/FlexNetOS/usr/bin`:

```bash
/home/flexnetos/FlexNetOS/usr/bin/git-kb status --json
/home/flexnetos/FlexNetOS/usr/bin/git-kb board --json
```

GitKB materializes checked-out documents under `.kb/workspaces/main/` in this
repository. The `.kb/store/`, `.kb/.cache/`, and `.kb/workspaces/` directories
are local ignored state.

Read `.kb/AGENTS.md` for the full GitKB workflow and task/context policy.
