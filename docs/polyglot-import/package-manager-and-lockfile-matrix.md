# Package Manager And Lockfile Matrix

| Ecosystem | Package markers | Lockfiles / dependency sources | Safe default rows | Default-path risk |
|---|---|---|---|---|
| Rust | Cargo.toml, workspace manifests | Cargo.lock, .cargo/config | workspace/package/target/profile/dependency rows | already covered by V1.1 specialization |
| Python | pyproject.toml, requirements files, tool config | uv.lock, poetry.lock, exported requirements | package manager, lockfile, dependency edge, config rows | avoid lock refresh or env sync by default |
| Ruby | Gemfile, gemspecs | Gemfile.lock | package, lockfile, dependency edge rows | avoid bundle install |
| JS/TS | package.json, workspace configs | package-lock.json, pnpm-lock.yaml, yarn.lock, bun.lock variants | package, workspace, lockfile, dependency edge rows | avoid install and generated caches |
| Go | go.mod | go.sum, vendor/modules.txt when present | module, dependency edge, vendoring rows | avoid go mod tidy or network fetch |
| Nix | flake.nix, default.nix, shell.nix | flake.lock | flake input, lockfile, package marker rows | avoid evaluation and build in default path |
| Generic config/docs | JSON/YAML/TOML/Markdown/HTML/CSS manifests | inline references only | config/build/doc marker rows | meaning is schema-specific |

## Rules

- No package-manager dependency installation by default.
- No downloaded runtimes or package caches in default tests.
- Package and lockfile facts are captured as metadata and dependency rows before
  any deeper semantic analysis is attempted.
