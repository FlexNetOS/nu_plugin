# Package Manager And Lockfile Matrix

| Ecosystem | Package markers | Lockfiles / dependency sources | Safe default rows | Default-path risk |
|---|---|---|---|---|
| Rust | Cargo.toml, workspace manifests | Cargo.lock, .cargo/config | workspace/package/target/profile/dependency rows | already covered by V1.1 specialization |
| Python | pyproject.toml, setup.py, setup.cfg, requirements/constraints files, Pipfile | uv.lock, poetry.lock, Pipfile.lock, pinned/exported requirements | package-manager marker, project/build metadata, lockfile provenance, dependency-specification, config rows | never execute setup/build backends or refresh, sync, lock, or install by default |
| Ruby | Gemfile, gems.rb, gemspecs, .ruby-version | Gemfile.lock, gems.locked | package-manager marker, declared dependency, locked specification/edge, source, platform, and config rows | never evaluate Ruby/Bundler DSL or run Ruby, Bundler, RubyGems, gem builds, or Rake by default |
| JS/TS | package.json, workspace configs | package-lock.json, pnpm-lock.yaml, yarn.lock, bun.lock variants | package, workspace, lockfile, dependency edge rows | avoid install and generated caches |
| Go | go.mod | go.sum, vendor/modules.txt when present | module, dependency edge, vendoring rows | avoid go mod tidy or network fetch |
| Nix | flake.nix, default.nix, shell.nix | flake.lock | flake input, lockfile, package marker rows | avoid evaluation and build in default path |
| Generic config/docs | JSON/YAML/TOML/Markdown/HTML/CSS manifests | inline references only | config/build/doc marker rows | meaning is schema-specific |

## Rules

- No package-manager dependency installation by default.
- No downloaded runtimes or package caches in default tests.
- Package and lockfile facts are captured as metadata and dependency rows before
  any deeper semantic analysis is attempted.

## Python Package And Lockfile Coverage

Python package files are parsed as inert data from the captured snapshot.
Discovery records every recognized file and the parser result independently so
that malformed or partially supported metadata remains visible.

| Surface | Static facts to capture | Explicit boundary or capture gap |
|---|---|---|
| `pyproject.toml` | `[project]` identity, declared Python requirement, dependency and optional-dependency specifications, entry-point text, `[build-system]` requirements/backend text, and recognized tool-table markers | do not import or invoke the build backend; unknown tool tables remain namespaced config facts |
| `setup.cfg` | recognized metadata, options, dependency text, package-data markers, and entry-point text | configuration interpolation or plugin behavior is not executed |
| `setup.py` | file identity and executable-build-script marker | never execute or claim complete metadata from arbitrary Python code; emit a static-analysis gap unless a separately qualified parser extracts bounded syntax facts |
| requirements and constraints files | include/constraint directives, requirement text, pins/ranges, markers, extras, hashes, index/find-links directives, editable/local/VCS references, and source location | do not follow remote references or files outside the admitted snapshot; unresolved includes and unsupported syntax become gaps |
| `uv.lock` | schema/version marker, package identity/version/source text, dependency relationships, resolution markers, and lockfile digest | do not run `uv lock`, `uv sync`, or `uv export`; unsupported schema versions retain raw provenance plus a typed gap |
| `poetry.lock` | lock format/version markers, package identity/version/source text, dependency relationships, groups/markers when represented, and lockfile digest | do not run Poetry or rewrite the lock |
| `Pipfile` / `Pipfile.lock` | declared sources, Python-version markers, dependency groups/specifications, lock metadata, resolved package/version/hash text, and file digest | do not run Pipenv, resolve sources, or refresh the lock |

Dependency rows must distinguish declared specifications from locked
resolutions and retain group/extra/marker/source context. The importer must not
merge conflicting files into a fabricated single environment: each fact keeps
its originating file/blob and package-manager format. Local path and file
references are normalized only within the admitted snapshot; credentials in
URLs or tool configuration are redacted under the existing secret policy.

The `python-packaging` fixture set covers a valid example of every row above,
malformed TOML/JSON and requirements syntax, duplicate/conflicting declarations,
unknown lockfile schema versions, missing local includes, URL credential
redaction, and stable ordering. Validation proves identical output across
repeated offline runs and verifies that no package-manager command, build
backend, project script, network access, cache write, virtual-environment
creation, or dependency installation occurs.

## Ruby Package And Lockfile Coverage

Ruby package surfaces are captured from the admitted snapshot as source or
inert lockfile data. `Gemfile`, `gems.rb`, and `.gemspec` files are executable
Ruby DSLs: the safe importer may record bounded syntax and recognized literal
calls, but it must not evaluate them or claim that static observations are a
complete resolved package definition.

| Surface | Static facts to capture | Explicit boundary or capture gap |
|---|---|---|
| `Gemfile` / `gems.rb` | file identity, source declarations, literal `ruby` constraints, literal `gem` name/version/options, groups, platforms, git/path/plugin-source text, and source location when statically recognizable | do not evaluate the Bundler DSL, interpolate environment values, load included Ruby files, contact sources, or infer computed arguments; nonliteral or unsupported calls produce bounded syntax facts and a gap |
| `.gemspec` | file identity plus statically recognizable literal gem identity, version, files metadata, required Ruby/RubyGems versions, and runtime/development dependency calls | never load or build the gemspec; dynamic values, filesystem enumeration, command substitution, and arbitrary Ruby remain executable-source gaps |
| `Gemfile.lock` / `gems.locked` | lockfile digest; `GEM`, `GIT`, and `PATH` source provenance; remote/revision/branch/ref text; locked specs and dependency edges; `PLATFORMS`, `DEPENDENCIES`, `RUBY VERSION`, `BUNDLED WITH`, checksums, and recognized extension sections | do not run `bundle lock`, `bundle install`, or normalize/rewrite the lock; preserve unknown sections and emit a typed gap for unsupported format variants |
| `.ruby-version` | exact declared runtime-version text and file provenance | marker only; do not select, download, or execute that Ruby version |
| `.bundle/config` and admitted Bundler config | recognized key names and redacted, policy-safe values with file provenance | do not merge host/user configuration or expose credentials; ignore configuration outside the admitted snapshot and redact credential-bearing source keys |

Declared dependency rows and locked specification rows remain distinct.
Dependency edges retain requirement text, group/platform context, source kind,
and originating byte/blob provenance. Multiple Gemfiles, gemspecs, or lockfiles
are not merged into a fabricated environment. Git and path sources remain
references unless their targets are already admitted snapshot files; remote
contents are never fetched. Native-extension presence may be recorded as a
build-risk marker but no extension is compiled.

The `ruby-packaging` fixture set covers literal and computed DSL forms, runtime
and development dependencies, grouped/platform-specific gems, registry/git/path
sources, lockfile aliases, multiple platforms, checksums, Ruby and Bundler
versions, malformed and unknown lockfile sections, missing local paths,
conflicting declarations, credential redaction, and stable ordering.
Validation proves identical output across repeated offline runs and verifies
that no Ruby, Bundler, RubyGems, Rake, gem build, project script, network
access, cache write, runtime installation, lock refresh, or dependency
installation occurs.

## JavaScript And TypeScript Package And Lockfile Coverage

JavaScript and TypeScript package surfaces are parsed as inert snapshot data.
Discovery records each manifest, workspace declaration, config, and lockfile
independently. The importer does not select one package manager, merge nested
projects into a fabricated environment, or infer installed state from a
lockfile or cache directory.

| Surface | Static facts to capture | Explicit boundary or capture gap |
|---|---|---|
| `package.json` | file digest; package name/version/private/type; engines and package-manager declarations; dependency groups with exact specification text; workspaces; exports/imports/browser/bin/types/main/module metadata; and script names with redacted, non-executed values | do not run lifecycle or package scripts, load package code, resolve conditional exports, or treat dependency declarations as installed/resolved packages; malformed or unknown fields remain provenance plus diagnostics |
| npm `package-lock.json` and `npm-shrinkwrap.json` | lockfile version; root metadata; package paths; names/versions; resolved-source kind with credential redaction; integrity text; dependency edges; link/dev/optional/peer flags; and lockfile digest | support recognized v1/v2/v3 structures without invoking npm or rewriting the lock; unknown versions, missing targets, or inconsistent edges produce typed gaps |
| pnpm `pnpm-lock.yaml` and workspace declarations | lockfile version/settings; importers; declared specifications; package/snapshot identities; dependency and peer edges; patched/local/workspace references; integrity/source text; workspace globs; and file digests | do not run pnpm, expand globs outside the admitted snapshot, fetch the store, apply patches, or infer peer resolution beyond represented lock data; version-specific unsupported fields remain namespaced facts plus gaps |
| Yarn classic and Berry `yarn.lock` | format/metadata marker; descriptor and locator text; version/resolution/checksum; dependency and peer edges; link/portal/patch/workspace/protocol references; cache-key metadata when present; and lockfile digest | parse classic and Berry formats as distinct versioned dialects; do not execute Yarn, plugins, constraints, Plug'n'Play loaders, patches, or cache archives; unsupported grammar or plugin-defined meaning produces a gap |
| Bun `bun.lock` and `bun.lockb` | format/version marker when observable; workspace/package identities; dependency edges; source/integrity text; and exact lockfile digest/bytes | parse supported text lock versions as inert data; preserve binary lockfiles as Tier 0 evidence unless a qualified bounded decoder exists; never run Bun or convert/rewrite a lockfile |
| workspace/config surfaces | npm/Yarn `workspaces`, `pnpm-workspace.yaml`, recognized Bun workspace metadata, nested package boundaries, `tsconfig*.json`, and `jsconfig*.json` references/options as source text | resolve local config/workspace references only within the admitted snapshot and within depth/file limits; do not load executable JS/TS configs, plugins, environment-derived config, or build referenced projects |

Declared specifications, lockfile resolutions, workspace membership, and local
installed artifacts are separate fact classes. Every row retains its
originating file/blob, package-manager dialect/version, source location, and
applicable dependency group or flags. A repository containing multiple or
conflicting lockfiles reports each one and an ambiguity gap; timestamp,
filesystem layout, or host cache state must not be used to choose a winner.
Local `file:`, `link:`, `portal:`, `workspace:`, and patch references are
normalized only when their targets are within the admitted snapshot. Remote
content is never fetched, and credentials in registry, git, tarball, or config
URLs are redacted under the existing secret policy.

The `javascript-typescript-packaging` fixtures include nested npm, pnpm, Yarn,
and Bun workspaces; dependency/dev/optional/peer combinations; aliases,
overrides/resolutions, scoped packages, conditional exports, local and patched
references; npm lockfile versions; pnpm importer/snapshot variants; Yarn classic
and Berry syntax; Bun text and binary lock evidence; malformed and unknown
formats; missing local targets; conflicting manifests/lockfiles; and credential
redaction. Validation proves stable bounded ordering across repeated offline
runs and verifies that no runtime, package-manager, compiler, bundler, plugin,
project script, network access, cache write, lock refresh, dependency
installation, or project build occurs.
