# The mod's crate

The root `Cargo.toml` and `src/` form the mod's crate. It depends on `sunrise-mod-api` (lib
`mod_api`), never directly on the game. The build supplies the pinned SDK and declared
[mod dependencies](../../concepts/dependencies.md).

**Registry dependencies belong to the mod.** A mod may use crates.io packages that the game does
not use, or different versions of packages the game uses. Each package must have an exact version
and SHA-256 checksum in the mod's registry dependency lock, stored as `Cargo.lock` beside its
`Cargo.toml`. Keep this file in version control and include it in the mod package. Changing it
changes the native build's cache identity.

`cargo sunrise-mod build` writes it the first time a mod with registry dependencies is built. After
you change `Cargo.toml`, resolve again on purpose:

```sh
cargo sunrise-mod build --update-lock
```

A build server writes the lock with the `native-lock` command of the
[`native-build` contract](../../reference/sdk/08-native-build.md).

Writing the **registry dependency lock** creates an inert copy of the manifests. It removes SDK
entries at their exact pinned SDK paths and resolves registry metadata without copying or executing
mod sources, build scripts, or proc macros. Existing pins are not replaced without `--update-lock`;
a refused input leaves the existing lock untouched. A starter that uses only the SDK does not
need a lock until it adds a registry dependency.

A registry build validates and rewrites the manifests in its actual Cargo staging folder. It
collects requirements from the mod and its reachable local crates, including build, dev and
target-specific dependencies, and checks them against the mod's own lock before resolving. Cargo
resolves registry **index metadata** using that staged workspace's aliases, features and SDK-owned
patches. Every resulting registry identity and checksum must match a supplied lock before the
build fetches crate archives. Cargo checks the downloaded archives against those checksums and
compiles the workspace **offline**. The build does not edit the mod's original manifests or lock.

The game's own packages never change because of a mod. The game ships prebuilt, and a mod's registry
packages are compiled for the mod and linked into the mod's library, so a package both use exists
twice: the features and dependency versions of the mod's copy are the mod's own. What a build checks
is that every registry package it compiles is pinned, by version and checksum, in the game's lock or
in the lock of a mod in the build.

All native mods of an instance compile in one Cargo workspace, which resolves one version of a
package for every mod that uses it. When two mods' locks pin different versions that both their
requirements allow, both mods are built on one of the two. So a mod may be built on another
semver-compatible version of a dependency than the one its author locked; a requirement that must
not move is written as an exact one (`dtoa = "=1.0.9"`).

Each mod is its own library, and a library carries a copy of every registry package linked into it,
so a mod depending on two mods that each use `serde` would reach two copies of `serde`, which Rust
refuses to link. When two or more mods of a build link registry packages, the build links them into
one more library instead, `sunrise_registry_shared`, which those mods depend on and which is
installed beside them: a mod is not registered by it, and loads it like any library it depends on.
It takes every package those mods link at run time, as Cargo resolved them for the target (proc
macros and build-script dependencies stay out), and nothing else changes: each mod's requirements,
features and lock still decide what Cargo resolves and the build checks. A build where one mod or
none uses registry packages writes no such library. The package name `sunrise-registry-shared` and
the crate name `sunrise_registry_shared` are reserved for it.

`native-lock` starts from the game's `Cargo.lock`, which it downloads once from the pinned game
commit. A package the game also locks takes the game's version where the mod's requirements allow
it, so mods made for one game build agree on those packages; the mod's other packages take the newest
version that fits. A requirement the game's version can't meet (`libc = "0.2.999"`) gives the mod
its own version of that package.

A package the SDK builds from its own patched source (the `[patch]` table of the SDK's
`engine/Cargo.toml`) is used in that form only: at the SDK's version a mod's graph gets the SDK's
source, never the registry's. A native library with conflicting providers cannot be silently
substituted.

Git dependencies, alternate registries, paths escaping the mod, linked dependency paths, mod-owned
`[patch]`, `[replace]`, `[workspace]`, and Cargo source configuration are refused. Name other mods in
[`sunrise-mod.toml`](../../concepts/dependencies.md), not with a path in `Cargo.toml`; the build
adds their edges after it checks the authored paths. Registry dependencies remain native code:
their build scripts and proc macros execute during compilation, and the resulting mod runs with
your rights. Trust their authors as well as the mod's author.

`cargo sunrise-mod build` compiles these dependencies without asking: you chose them, and their build
scripts and proc macros run with your rights. A player's launcher asks first: it shows how many registry
crates the mod adds and names the reachable build scripts and proc macros, and declining stops before any
mod dependency is compiled. Downloading a checksummed crate does not make its code trusted.

For native builds, Cargo compiles the mod's registry graph and the final wrapper
against immutable SDK binaries. Registry dependencies never cause an automatic source rebuild.
The SDK must include its locked graph and runtime native-library inventory; SDKs without
that information refuse registry builds. The build preserves the compiler/cache wrapper chain
and uses the SDK's pinned Rust compiler.

**SDK crate identities are not interchangeable.** A registry-built copy of a trait or type is not
the SDK's copy, even at the same version. For example, passing an SDK value implementing the SDK's
`serde::Serialize` to a registry-built `serde_json` can fail with Rust E0277. The build reports
that boundary; it does not replace either library or rebuild the game to hide it. Keep private
registry types behind your mod's API-facing values.

Runtime native libraries emitted by registry builds are collected only from managed build and
Cargo directories. Host-only build tools and proc macros are not packaged into the game. Conflicting
native providers or output filenames, escaping outputs, relative native search paths, and bundled
framework outputs are refused explicitly. System libraries remain system dependencies. These
restrictions do not trigger a source fallback.

## Build restrictions

A mod's own build script is refused with `E_NATIVE_BUILD_SCRIPT`: ship generated source instead.
`E_NATIVE_FEATURES` refuses mod feature declarations and custom SDK feature requests; use the
pinned SDK's feature set. These restrictions do not prohibit the separately approved build scripts
of registry dependencies.

The build adds a small generated module to the crate's root, the library entry that the game
loads, and that module exports two `#[unsafe(no_mangle)]` functions. A crate root may say
`#![deny(unsafe_code)]`, which the module overrides for itself alone, but not
`#![forbid(unsafe_code)]`, which nothing can override: such a mod is refused by name before it
compiles.
