# Ingestion round-trip fixture — Nushell module with exported and private defs.
# The em dash above and this ünïcödé comment prove multi-byte exactness.

export def greet [name: string] {
    $"LifeOS greets ($name)"
}

def helper [] {
    42
}
