# Design: mod datapack registries

> Historical: this design predates the removal of WebAssembly mods.
> See [`DatapackRegistriesRegistrar`](../../../crates/sunrise-mod-vocab/src/parts/datapack_registries.rs) for the current native API.
> The current registrar declares a schema and the game decodes entries. The proposals below are
> historical; see [Stage 3 progress](#stage-3-progress) for implemented local staging and reads, and
> [Stage 4 progress](#stage-4-progress) for implemented network publication.

Provisional contract for [#811](https://github.com/arismoko/horizon/issues/811), not an implemented API. Ari's side posts the decision on the issue. This note files no issues or PRs.

## Decision: decoder and generation ownership

The declaring mod owns the typed codec and its schema. The game owns decoding orchestration, candidate storage, reference resolution, validation, generation assignment and atomic publication. A decoder never installs values into live state. The runtime retains its callable only while the declaring mod is loaded; unloading requires closing its worlds first. No raw native pointer or wasm guest address is a registry value in host storage.

Propose one load-time registrar, corresponding to DataPackRegistryEvent.NewRegistry, with the appropriate Host and a scoped registrar. Illustrative declaration: `example:spell`, type `Spell`, data codec `{power: integer 0..100}`, optional network codec and explicit client requirement. Names and signatures await API review. Require namespaced registry keys owned by the declaring mod; reject a second declaration even with an identical codec, reporting both owners. Freeze declarations after dependency-ordered loading. Other mods and packs may contribute entry names in their own namespaces. Entry path for `example:spell`, entry `addon:fire`, is `data/addon/example/spell/fire.json`; pin this proposed mapping in tests before implementation. The namespaced registry folder is NeoForge's rule for datapack registries outside `minecraft` (its Registries guide, section "Datapack Registries"); vanilla's own registries use the registry's path only, and registration refuses `minecraft` keys, so the vanilla form never applies to a mod's registry.

Codecs consume bounded immutable data plus a candidate registry lookup, returning a typed immutable value or structured error. No world mutation, I/O, retained Host, scheduling or active-generation lookup during decoding. Resolve references against the complete candidate graph, including forward references; reject unresolved references and unsupported cycles. Use one generated codec/value representation for native and wasm, with typed reads through registry handles. Erasure is internal and checked against the declaration's type token; no unchecked downcasts or guest memory retained after calls.

## Evidence and limits

Vanilla 26.3 was inspected only through `cd /workspace/sunrise && python3 tools/rea_vanilla.py method net.minecraft.resources.RegistryDataLoader load --overload 0`. It accepts context lookups, registry descriptors and an executor, creates resource-manager load tasks per descriptor, and delegates to the common loader returning a future of frozen registry access. This supports descriptor-driven candidate loading; it does not prove arbitrary mod registration, reload scope or failure atomicity. Those are Sunrise proposals here. Vanilla caller coverage and the distinction between world-generation registries and reloadable registries still need oracle fixtures before implementation.

The [public registry guide](https://docs.neoforged.net/docs/concepts/registries/) was reachable but labels itself **26.1**. It is background only, not proof of 26.3 behavior. The [26.3 NewRegistry Javadoc](https://aldak.netlify.app/javadoc/26.3.x/net/neoforged/neoforge/registries/DataPackRegistryEvent.NewRegistry.html) was requested and inaccessible through the reader. Exact overloads, network codec semantics, absent-mod behavior, reload guarantees and failure policy are **unverified**. The issue names the counterpart and asks for declarations and entry decoders. No NeoForge source, patches or other project's source was read. Do not advertise exact counterpart equivalence until public documentation verifies it.

The public NeoForge 26.3 changelog (maven.neoforged.net, `neoforge-26.3.0.102-changelog.txt`) and the pull requests it names (GitHub descriptions only; no source read) add what the 26.1 guide could not: `26.3.0.20-beta` "[26.3] Add support for registering custom reloadable datapack registries" (#3527, with a cleanup of the registration event and hooks), `26.3.0.44-beta` "[26.3] Add dedicated builder for datapack registries" (#3572, whose first option is a `RegistryValidator` for the registry), `26.3.0.58-beta` "[26.3] Gather Datapack Registry Entries Before Data Providers" (#3587) and `26.3.0.88` "[26.3] Improve datapack registry definition storage" (#3659: world, reloadable and synced registry definitions are all kept in lists mods can read). So NeoForge 26.3 distinguishes world registries (read at world load) from reloadable ones, which the earlier open question about which categories reload anticipated. Sunrise's custom registries reload with `/reload` and are validated by their schema and bounds, so they correspond to the reloadable kind with a built-in validator; Sunrise has no world-load-only kind, and adding one would be a separate decision. The Javadoc for the builder's exact methods was still unreachable, so no signature is claimed.

## Decode, commit and retained identity

Decode after pack selection and overlay precedence have selected one winning file per entry, before any live table or network publication. Reuse existing pack conditions and overrides; never invent another pack scanner. Include prepared base data even when there are no overlay files: the early return in build_world_tables must not skip declared registries. Report mod, registry, entry, pack, file and field path for invalid data. Duplicate declarations are registration errors; multiple pack definitions are ordinary priority overrides. Invalid winning data must fail, never fall back to a lower-priority value. Initial failure prevents world admission. Reload failure retains all old tables, registry values, generations and client snapshots.

Use a world/session identity plus monotonically increasing u64 registry-set generation. Initial successful publication is generation 1, every successful reload increments once, including identical content. Refuse exhaustion instead of wrapping. Failed reload does not increment. A handle contains session, registry type, generation and entry identity. All access and use checks them against the active set: generation 1 handles and held entry wrappers return StaleGeneration after generation 2, even if names and slot numbers match. Deleted entry lookup returns MissingEntry. Closing and reopening a world invalidates session identity. A retained handle must validate on every read. A successful read lends the entry only to a visitor, pinning that generation without holding a registry lock during the callback. Explicit copied plain data is a snapshot, carries no live entry authority, and cannot be passed as a current entry. Persist names, never session slots; re-resolve explicitly after load.

Prepare codecs, references and derived tables before commit. Publish the registry set with WorldTables and dependent data under one coordinator at a safe server boundary. The implemented registry path encodes negotiated snapshots after commit; see Stage 4. Consumers cannot observe mixed generations. Existing reload-listener apply callbacks run before world preparation today; implementation must move fallible preparation before effects or constrain callbacks to candidate-local effects, rather than claiming existing ordering provides rollback. Listener panic or guest trap aborts the candidate; no partial publication. Catch native unwind at the boundary, disable/report the failing callable under runtime policy; process-aborting panics cannot be recovered.

## Verified Sunrise route owners

Paths and lines are inspected at base `8b736c3cbc`; these identify integration owners, not implementation claims.

| Route | Owner and required treatment |
| --- | --- |
| World open, including local/integrated and dedicated Game entry | `crates/sunrise-client/src/game.rs:11805` selects packs; line 11808 calls build_world_tables. Prepare generation 1 here before admission. |
| Shared table builder | `crates/sunrise-client/src/game/world/world_entry.rs:218` owns build_world_tables and overlays; line 511 reuses it for reload. Add candidate registries to its prepared result. |
| Explicit reload and datapack selection | `crates/sunrise-client/src/game/administration/functions.rs:208` owns reload_datapacks; line 948 owns cmd_reload; lines 253 and 278 prepare/install world tables. All run_load authoritative routes share this coordinator. |
| Content inventory changes | `crates/sunrise-client/src/game/modding/mods/mod.rs:1329` calls reload_datapacks(true). Declaration changes cannot sneak through this route: require a new mod-load session; data-only changes use normal reload. |
| Additional content refresh | `crates/sunrise-client/src/game/administration/functions.rs:1281` prepares listeners and line 1301 prepares world reload. It must use the same candidate/commit boundary, not a second generation allocator. |
| Table publication | `crates/sunrise-client/src/game/world/world_entry.rs:337` owns install_world_tables; line 505 owns prepare_world_reload, including wire preparation. Extend both, keeping failure before publication. |
| Reload callbacks | `crates/sunrise-client/src/game/world/server_reload/mod.rs:52,100,117` own prepare, apply callbacks and store admission. Enforce the candidate ordering described above. |
| Remote client | reload_datapacks at `crates/sunrise-client/src/game/administration/functions.rs:251` excludes remote worlds from authoritative preparation. Remote clients receive server candidates; never decode local datapacks as server authority. |
| Join negotiation and send | `crates/sunrise-client/src/lan/conn.rs:1048,1084,1150` collect/decode/compare mod lists; line 1222 sends RegistryData. Extend negotiation before sending custom entries. |
| Client resource reload | `crates/sunrise-client/src/game/world/packs/mod.rs:240` owns reload_packs. Resource-only refresh must not advance authoritative datapack generations. |

This is the inspected production route inventory; tests also call the shared functions directly. Exact receive-side configuration commit and all vanilla callers remain implementation trace prerequisites; packet tests alone are not proof of client publication coverage.

## Clients and absence negotiation

Propose three explicit modes: server-only (no network codec), synchronized-required, synchronized-optional. A network codec defines a bounded client projection, which may omit server secrets. Negotiate registry key, declaring mod, schema fingerprint and mode before any payload. Required missing mod or incompatible codec refuses join with a precise reason before entry decoding. Optional absent client mod receives no payload and gains no fabricated empty registry; server gameplay may not require that client's interpretation. Server-only registries do not require the client mod merely because they exist. Reconcile this with current mod-list comparison so blanket mismatch refusal does not silently defeat optionality; preserve unrelated dependency requirements.

Server is authoritative. Send a full sorted snapshot initially and after reload with session, generation, entry keys and bounded projected values. Client decodes and validates off to the side, acknowledges readiness, then activates at the agreed boundary; generation-tagged traffic is held until activation. Failed required-client decode disconnects that client without exposing a partial set; successful server reload need not roll back for a disconnected peer. Never send generation-2 references to a peer still using generation 1. Late joins receive only the current complete generation. Network numeric slots are scoped to this snapshot and never persisted. Transport and configuration re-entry details require protocol review; do not create a parallel registry packet store.

## Existing registrars, runtime, and cost

Existing register_* hooks declare built-in content or codec types at mod load; they do not become per-reload callbacks. The new registrar declares the registry schema once; loading its JSON happens each candidate build. add_server_reload_listeners remains for independent resource consumers, not a second way to declare this registry or publish entries. Data maps annotate existing entries and remain a separate feature.

`crates/sunrise-mod-runtime/src/registry_keys.rs:6` owns load-time Requests and line 18 owns Names, including requested slots and resolved maps. These tables are name/slot machinery, not typed datapack value storage or a reload generation authority. Reuse their validated key resolution where appropriate; place immutable per-world generation sets under the world coordinator. Keep mod-runtime engine-independent. Generate native and wasm registration, codec calls, typed reads and refusal errors together; both use the same dependency ordering and semantics. Native exact-build checks and wasm interface checks remain their existing admission rules.

Treat files, decoder results and network bytes as untrusted. Validate local identifiers, field values and references against the registry schema and the complete candidate. Network bounds apply to the data sent to a peer, not to local candidate admission. Refuse only custom registry keys owned by this feature; unrelated unknown-key behavior remains with its existing owner.

Resolve declarations once. With none, skip custom scanning, codec invocation, candidate allocations and synchronization extensions; no per-tick mod scans. Reads validate session/generation in constant time and perform indexed lookup. Reload work scales with selected entries and references, not ticks. Measure the empty declaration path and native/wasm bounds independently.

## Acceptance matrix for implementation

Expected values below are proposals, not tests reported passing.

| Fixture | Expected value |
| --- | --- |
| Test mod declares example:spell; base fire power=3, higher pack power=7 | Typed fire.power=7, generation=1, exactly one selected entry. |
| Reload override to power=9 | New read=9, generation=2; reading the held generation-1 handle returns StaleGeneration without visiting. |
| Winning override power=101 or malformed JSON | Reload fails with pack/file/field error; power=7 and generation=1 remain; no snapshot sent. |
| Two mods declare example:spell | Admission fails with both owner names; zero published custom registries. |
| Entry refers forward to addon:ice, power=4 | Resolves after candidate completion; absent ice refuses entire candidate. |
| Remove fire on successful reload | Fresh lookup=MissingEntry; old handle=StaleGeneration; generation=2. |
| Identical successful reload; failed reload afterwards | Generation advances 1 to 2 once; failure leaves 2. |
| Close world then reopen with same entry and slot | Reading the old handle returns WrongSession; new generation=1 with distinct session. |
| Required client mod missing/schema mismatch | Join refused before payload; zero custom entries installed. |
| Optional client mod absent; server-only registry | Zero bytes for omitted registries, no empty fake registry; join succeeds if other requirements pass. |
| Present native and wasm client, power projection=7 | Both read 7 in generation 1; activation after full decode only. |
| Client reload decode traps | Zero partial entries; client disconnected; server retains committed generation 2. |
| Valid local entry padded with more than 1 MiB of JSON whitespace | The entry loads with the same decoded value. |
| Native unwind or wasm decoder trap | Candidate discarded, no sends, old value=7; next valid candidate can publish. |
| No declaration; resource-only reload | Zero custom decode calls/allocations/sends; datapack generation unchanged by resource refresh. |
| Every authoritative route above with override=9 | Exactly one publication and generation increment; remote local packs cannot replace server value. |

## Proposed stages

File none. Stage 1: obtain exact public 26.3 contract and vanilla load/reload caller fixtures; settle the decisions below. Stage 2: generated registrar and bounded codec representation for native and wasm, duplicate/type validation and test-mod fixtures. Stage 3: candidate ownership, generation-checked reads and common atomic coordinator across every listed route, including listener failure. Stage 4: negotiation, snapshot transport, receive-side activation and absent-mod fixtures. Stage 5: acceptance matrix, route parity, adversarial limits and empty-path allocation measurements. No stage authorizes Rust work in this documentation task.

## Stage 3 progress

`DatapackCandidate::build` (`sunrise-mod-vocab`, `world/datapack_registry_set.rs`) accepts
pack-ordered versions of files the caller has already selected and admitted. It decodes winners
without running their conditions and checks references across the complete candidate. Forward references
work; a missing target in a declared registry refuses the candidate, and a reference to an undeclared
registry is left to its owner. `DatapackRegistrySet::from_candidate` turns a valid local candidate into
a set without encoding a snapshot. The game stages and builds that set in `build_world_tables`. It
installs the first set via `open_world_tables`; reload routes use `prepare_world_reload` and publish
one replacement through `install_world_tables`. Failed preparation does not advance the published
generation. A remote client does not publish local server registries.

Mods use `level.datapack_entries(host, registry)` for sorted owned entry IDs and
`level.datapack_read(host, registry, entry_id, &mut visit)` to borrow one entry directly by
`(registry, entry_id)`. A known empty registry has an empty list; an unavailable level or
registry has no list. On the logical server, both the active dimension and any loaded parked
server dimension are accepted (matching vanilla `Level.registryAccess()` on `ServerLevel`); on a
client only the active client dimension is accepted. `UnknownRegistry` and `MissingEntry` are
refusals, not successful visits. A stale epoch, unloaded dimension, unavailable registry, or
absent publication returns `UnknownRegistry`; a host without a backend returns `Unavailable`.

The read pins the set and drops any publication lock before visiting the same `DatapackEntry`
stored there. The visitor is called once on success and never on refusal. It can read fields in
schema order or use `entry.get(name)`. It cannot retain the borrow; a mod keeps IDs for a new
lookup or explicitly copies the fields it needs as a snapshot.

NeoForge's `Registries.registryDirPath` (`CommonHooks.prefixNamespace`) uses `<path>` for a
`minecraft:<path>` key and `<namespace>/<path>` for other registry keys; entries and tags share that
rule. A physical path in two registry directories is an entry in both, relative to each directory.
As in vanilla's `ResourceManagerRegistryLoadTask.load`, pack precedence selects the winner before
it is read or decoded. An unreadable winner reports `DatapackLoadReason::ReadFile(message)` with its
pack and file; an unresolved reference names the pack supplying the referring entry. A false
condition omits the winner without exposing a lower pack's file. Local loading has no per-file or
whole-candidate byte ceiling and no registry or entry count ceiling borrowed from network transport.
Unknown schema keys still fail; duplicate registry declarations still fail, rather than using pack
precedence to choose a declaration.

Like NeoForge's `ICondition` and `ConditionalOps.ConditionalDecoder`, the loader decodes the whole
condition array before testing and stops testing once the answer is known. `RegistryDataLoader.load`
uses current registry lookups. `RegisteredCondition.test` checks a bound holder;
`TagEmptyCondition.test` asks `ConditionContext.getTag` for resolved members. Static holders and
resolved pending static tags are available before registry codecs. Mod-declared registries also see the
candidate's frozen world data and previously bound dependency batches. A batch binds entries and tags
before consumers run; reads within the batch (self-reads or cycles) do not see its pending holders as
bound. Only typed built-in reads order those dependencies. Registry codecs use the vanilla feature
set; recipes and other downstream readers receive the completed candidate and the world's enabled
features, not an earlier published generation.

Ordinary `Host` queries still see published data. A registered `LoadCondition` receives the raw
JSON arguments, not a world handle. Its panic names the registering mod and condition in the load
error, is logged and queues an error toast, and disables only that handler for the session. Returned
errors and phase refusals are reported without disabling it. Loader-specific error handling decides
whether to refuse the candidate or skip an offending file; see
[Conditional data](../../modding/src/how-to/items/conditional-data.md#errors-by-loader).

## Stage 4 progress

NeoForge's `NewDatapackRegistryEvent.worldRegistry` and `reloadableRegistry` separate local loading
from network publication; `DataPackRegistriesHooks.addWorldRegistry` only adds a synchronized copy
with a network codec. In Sunrise, `ServerOnly` never sends entries and `Optional` sends them only to a
peer with the matching declaration. A server-only package can use either mode subject to the rest of
the join-content rule. `Required` needs a both-side package and a both-side supplier for a `served_by` namespace; a client-only package cannot declare a
datapack registry at all.

`DatapackRegistrySet::snapshot` projects the synchronized registries that a peer negotiated into a
`DatapackSnapshot` (`sunrise-mod-vocab`, `world/datapack_snapshot.rs`): keys, schemas, tags and
entries sorted by ID. The Wire offer and snapshot codecs bound the actual sent bytes.
`DatapackSnapshot::decode` uses `TooLarge` for an oversized encoded snapshot and `Malformed` for
invalid or trailing Wire bytes. `DatapackSnapshot::candidate` checks the decoded snapshot against the
client's declarations without a second size or collection limit. An unknown or repeated registry, a
mismatched schema, a missing required registry, a bad or repeated ID, an invalid field value, or an
unresolved reference or tag member each refuses the whole snapshot. An omitted registry is unavailable
to client reads rather than a fabricated empty registry. `DatapackRegistrySet::received` installs the
validated set on the client.

The join carries it (`sunrise-client/src/datapack_net.rs`). A Sunrise client that declared synchronized registries sends `sunrise:datapack_offer` (key and schema of each) after the mod list. The offer must fit vanilla's 32,767-byte serverbound custom payload; a failed encode ends configuration instead of sending no offer. Before any registry data the server answers on `sunrise:datapack_registries` with the snapshot of the registries both sides have, built by `snapshot_for`: a `Required` registry the client did not offer with the same schema ends the join with a reason naming the mod and the registry (a vanilla client, or a Sunrise one that sent no offer, is refused the same way), and an `Optional` one is left out with nothing sent. The client checks the snapshot with `candidate` while configuring; a snapshot that fails ends the join and nothing is kept. The received set is what a client mod reads through the same `LevelRef` methods. The visitor's borrow remains valid until it returns even if that received publication is cleared in the meantime; a new read checks the current publication.

After a reload commits the local set, the server uses each joined Sunrise client's original offer to
select and encode its negotiated snapshot, if any. Local commit does not guarantee that every
peer's projection fits. An encoding, transport or validation failure ends only the affected connection
with a named reason; the server's committed generation is not rolled back. The client swaps its set
only after complete reassembly and validation, just as at join.

What was left open in the first draft of this stage is settled by how the code is ordered, not by new code. (1) Activation versus play traffic: `install_world_tables` publishes the set and sends each joined Sunrise client its snapshot first, through the connection sender, which preserves packet order; what a mod sends to players is queued in `native_net.to_players` and leaves at the tick's end (`native_net.rs`). So on one connection the snapshot always precedes every mod message queued in the reload's tick. A client never sees a reload-tick message before the snapshot it refers to, so no holding queue is needed. (2) Reconfiguration: the declarations (registries, schemas, required or optional) come from the loaded mods and are fixed for the process; a reload changes entries only, so the negotiated set of registries cannot change under a joined client, and the snapshot a reload sends is built from the offer made at the join. A change of the mod list needs a new join, as for every other mod content. The native borrowed read does not change either the join or reload packet bytes.

Snapshot transport uses ordered fragments on the same custom-payload channel, for configuration and play alike. Each fragment starts with `DRS1`, a little-endian 32-bit total length and byte offset. The header and fragment data together fit vanilla's clientbound custom-payload limit (1 MiB). Totals above 128 MiB, changed totals, wrong offsets, duplicate starts and fragments with the wrong length are refused with a named reason. The receiver accumulates actual fragment data, not an allocation based on the total alone. The client keeps its previous set until the complete snapshot decodes and validates; configuration cannot finish with an incomplete transfer. Reconnect clears both the old set and any partial transfer.

Vanilla's Registry Data packets accumulate one registry per packet and validate at Finish Configuration; Known Packs omits data already held by the client. Neither mechanism specifies fragmentation of a single large custom payload. This channel keeps its explicit total/offset format and uses the 128 MiB default from NeoForge's `GenericPacketSplitter.receivedPacket` and `NeoForgeLocalConfig.splitPacketMaxSizeLimitMb` for the whole reassembled Wire snapshot. There are no separate count ceilings: vanilla's `ClientboundRegistryDataPacket.STREAM_CODEC` uses `ByteBufCodecs.list()` without a registry-specific count cap.

## Open questions

- What does the exact 26.3 counterpart guarantee? Recommendation: require reachable public Javadoc before implementation; retain explicit Sunrise proposals where public documentation is silent.
- Which registry categories reload? Recommendation: custom registries here always replace on reload; do not infer that vanilla world-generation registries also reload. Verify their distinction with oracle fixtures.
- Shared codec API with #813? Recommendation: use one generated bounded value/codec foundation, with separate registry lifecycle ownership; avoid a second transport representation.
- How should callback ordering become atomic? Recommendation: complete all fallible world and registry preparation before effectful listener apply, and require candidate-local writes until publication; review existing callback semantics before changing them.
- Are optional synchronized registries safe for all consumers? Recommendation: permit omission only through explicit declaration and negotiation; required gameplay interpretation must select required mode.
- Exact client receive and reload activation owner? Recommendation: trace configuration collector through world admission and reconnect/reconfiguration before coding transport, then add verified file:line owners and tests to this note.
