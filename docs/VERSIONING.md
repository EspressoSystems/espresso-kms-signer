# Versioning

The sidecar's job is to sign Espresso batches, and the batch encoding is owned by
[espresso-streamers](https://github.com/EspressoSystems/espresso-streamers), not by this
repo. So a release has to answer two independent questions, and the version number
encodes both.

## Two clocks

`x.y.z`:

- `x` **(major) — the streamer batch-format generation.** Bumped only when a new espresso-streamers version changes the `EspressoBatch` encoding the shape check
targets. A major bump means "this sidecar signs a different batch format" and always
ships as a coordinated deploy with the streamer and batcher.

- `y` **(minor) — sidecar changes.** New methods, config, fixes to our own code that do not change which batch format we support.
- `z` **(patch) — small fixes** within the same sidecar minor.


| Sidecar  | Streamer format             | Notes                                                            |
| -------- | --------------------------- | ---------------------------------------------------------------- |
| `v0.1.0` | none                        | the `eth_sign` sidecar, before the batch-format coupling existed |
| `v1.0.0` | `espresso-streamers v1.3.0` | first `espresso_signBatch` release                               |


Extend this table on every release.

## Single source of truth

The supported streamer version lives in exactly one authoritative place and is checked
against the fixtures it is generated from:

- `SUPPORTED_STREAMER_VERSION` in `[src/batch_shape.rs](../src/batch_shape.rs)` — the
version whose `EspressoBatch` shape this build signs.
- The pin in `[tests/fixtures/gen/go.mod](../tests/fixtures/gen/go.mod)` — the version
the wire fixtures are actually generated from.

CI (`Streamer version pin`) fails if the two differ, so the declared support and the
tested support can never drift apart.

## Finding what a deployment is

- **At runtime:** the startup log prints `version` and `supported_streamer`.
- **By tag:** each release is git-tagged with a GitHub release; the notes state the  
streamer version and the batcher-fork commit it deploys with.

## Releasing

1. If the streamer batch format changed: bump the espresso-streamers pin in
  `tests/fixtures/gen/go.mod`, bump `SUPPORTED_STREAMER_VERSION` to match, regenerate
   the fixtures (`cd tests/fixtures/gen && go run . -out ../`), and update
   `src/batch_shape.rs` if the shape itself changed. This is a **major** bump.
2. Otherwise it is a minor or patch bump.
3. Set the `Cargo.toml` version, tag it, and cut a GitHub release whose notes name the
  streamer version and the batcher-fork commit for the coordinated deploy.
4. A major bump never ships alone: the sidecar image, the streamer, and the batcher go
  out together, because the batch format is a contract shared by all three.

