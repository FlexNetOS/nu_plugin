# Proof And Round-Trip Gates

## Gate Set

| Gate | Requirement | Authority |
|---|---|---|
| P0 | research ledger complete | official-source research package |
| P1 | current-state audit complete | live repo truth surfaces |
| P2 | raw repo byte capture fixture passes | fixture evidence |
| P3 | language detection fixture passes | detector outputs |
| P4 | parser-backed summary fixture passes for baseline languages | parser evidence |
| P5 | package/lockfile matrix fixture passes | metadata and lockfile evidence |
| P6 | redb import/export manifest verifies | CodeDB manifest evidence |
| P7 | generated Rust crate builds | build evidence |
| P8 | single binary verifies embedded pack | embedded verification evidence |
| P9 | materialization proof matches allowed original bytes/metadata | round-trip proof |
| P10 | bounded Nu/CLI/MCP views pass | view and policy evidence |
| P11 | GitHub issue backlog or issue drafts exist | issue delivery evidence |

## Gate Interpretation

- A gate is only satisfied by direct evidence that matches its scope.
- Weakly consistent evidence is not enough.
- Negative-path checks must be part of the proof where safety boundaries matter.

## Required Round Trip

repo input
  -> import to CodeDB
  -> verify DB/export manifest
  -> generate export crate
  -> build single binary
  -> verify embedded pack
  -> materialize approved output
  -> compare bytes + metadata + policy

## Negative Cases

- raw source exposure through MCP
- unsafe overwrite during materialization
- dynamic runtime/build execution without approval
- secret leakage through exports, logs, or issue drafts
- schema drift that silently breaks Rust-first rows
