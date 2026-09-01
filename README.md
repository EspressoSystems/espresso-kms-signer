# espresso-kms-signer

A signing sidecar that signs L1 batch submission transactions via AWS KMS. It implements
the JSON-RPC signing protocol expected by `--signer.endpoint`, making it compatible with
any batcher that speaks that protocol (OP, Nitro, or otherwise). The private key never
leaves KMS hardware; the binary performs one KMS `Sign` call per request.

See the [design document](https://github.com/EspressoSystems/the-book/blob/main/op-stack-integration/kms-signing-sidecar.md)
for requirements and architecture rationale.

## Development environment

If you have [Nix](https://nixos.org/) with flakes enabled, `nix develop` drops you into a shell with the correct Rust toolchain and all system dependencies (OpenSSL, pkg-config, macOS SDK frameworks) already set up:

```bash
nix develop
cargo build
cargo test
```

Without Nix, make sure you have a recent stable Rust toolchain installed via [rustup](https://rustup.rs/) and run the same commands directly.

## Running locally (localstack)

```bash
# Start localstack community edition (the :latest tag requires a paid license)
docker run -d -p 4566:4566 localstack/localstack:3

# Create a secp256k1 KMS key
AWS_DEFAULT_REGION=us-east-2 AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test \
  aws --endpoint-url http://localhost:4566 kms create-key \
    --key-usage SIGN_VERIFY \
    --key-spec ECC_SECG_P256K1

# Note the KeyId from the output, then:
export AWS_KMS_KEY_ID=<key-id>
export AWS_ENDPOINT_URL=http://localhost:4566
export AWS_DEFAULT_REGION=us-east-2
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export CHAIN_ID=11155111          # Sepolia; use your devnet's chain ID
export LISTEN_ADDR=127.0.0.1:8547

cargo run
```

`cargo run` starts the JSON-RPC server and keeps it running in the foreground. At
startup it logs the derived Ethereum address, the address KMS signs as. This is now a
fully working signer: exercise it directly with the [Manual RPC calls](#manual-rpc-calls)
below.

To drive it from an OP batcher instead, leave the signer running and start the
**separate op-batcher process** with these two flags added to its own command (they
are op-batcher arguments, not something you run on their own), using the logged address:

```bash
op-batcher \
  --signer.endpoint http://127.0.0.1:8547 \
  --signer.address  <address from the startup log> \
  # ...the batcher's other flags
```

## Running in production (real KMS)

The binary reads standard AWS credential environment variables / instance profile.
Set only:

```bash
AWS_KMS_KEY_ID=<arn-or-key-id>
AWS_REGION=us-east-2
CHAIN_ID=<your-chain-id>
LISTEN_ADDR=127.0.0.1:8547        # localhost-only; cross-host deployments are not yet supported
```

Optionally restrict signing to a single BatchInbox address:

```bash
ALLOWED_TO=0xYourBatchInboxAddress
```

**Test keys:** create real KMS keys with a description, tags, and an alias so they can
be found and cleaned up later instead of accumulating as anonymous orphans:

```bash
aws kms create-key --key-usage SIGN_VERIFY --key-spec ECC_SECG_P256K1 \
  --description "espresso-kms-signer test key" \
  --tags TagKey=project,TagValue=espresso-kms-signer TagKey=purpose,TagValue=test
aws kms create-alias --alias-name alias/espresso-kms-signer-test --target-key-id <key-id>

# find and delete later (deletion needs a 7-30 day pending window)
aws kms list-aliases | grep espresso-kms-signer
aws kms schedule-key-deletion --key-id <key-id> --pending-window-in-days 7
```

## Manual RPC calls

With the signer running, you can call it directly with `curl`. In every example below,
replace `<address-from-startup-log>` with the exact address the signer logged at
startup, the `from` and `address` fields must equal it or the call is rejected:

```bash
# Health check
curl -s -X POST http://127.0.0.1:8547 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"health_status","params":[],"id":1}'

# Sign an EIP-1559 transaction (substitute the address logged at startup)
curl -s -X POST http://127.0.0.1:8547 \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_signTransaction",
    "params": [{
      "from":                 "<address-from-startup-log>",
      "to":                   "0xff00000000000000000000000000000011155111",
      "gas":                  "0x5208",
      "maxFeePerGas":         "0x3B9ACA00",
      "maxPriorityFeePerGas": "0xF4240",
      "value":                "0x1",
      "nonce":                "0x0",
      "chainId":              "0xAA36A7"
    }],
    "id": 1
  }'

# Sign an RLP-encoded Espresso batch (op-batcher's batch-signing path via
# `espresso_signBatch`). The payload is base64 of the batch's RLP encoding —
# the exact wire format op-batcher sends (Go's default JSON encoding of
# `[]byte`). The sidecar validates the payload is shaped like an encoded batch
# (an RLP list of four elements whose first element is a list) and computes
# the keccak256 digest itself; it never signs a caller-supplied digest.
# The example below is base64 of the minimal shape-valid payload [[],"","",""].
curl -s -X POST http://127.0.0.1:8547 \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "method": "espresso_signBatch",
    "params": [
      "<address-from-startup-log>",
      "xMCAgIA="
    ],
    "id": 1
  }'
```

`eth_signTransaction` returns a `0x`-prefixed RLP-encoded signed transaction ready to broadcast;
`chainId` must match `CHAIN_ID` (`0xAA36A7` = 11155111 for Sepolia). `espresso_signBatch` returns a
65-byte hex signature over keccak256 of the payload bytes (`r||s||v` with `v ∈ {0,1}`), with no
Ethereum message prefix — the digest espresso-streamers' recovery expects.

## Batch format compatibility

`espresso_signBatch` validates the **OP-stack** `EspressoBatch` encoding from
espresso-streamers (`op/derivation`), at the version pinned in
`tests/fixtures/gen/go.mod`. The Espresso network itself carries opaque namespace
bytes, so the batch format is a rollup-stack convention, not an Espresso one — and
this sidecar supports only the OP one. A Nitro chain uses a different encoding
(`nitro/` in the same repo) and is **not** supported by this build; adding a stack
means a new format module behind config selection, not a change to this check.

The shape check is deliberately shallow — outer structure only (an RLP list of four
elements whose first element is a list). Changes *inside* the batch (new header
fields, new transaction types) pass through with no sidecar change. Only a change to
the outer shape breaks it, and it breaks **closed**: the sidecar refuses to sign
rather than signing something it no longer recognizes. Outer-shape changes are
chain-level format migrations that already require coordinated batcher + streamer
deploys; the sidecar joins that same deploy.

Drift between this repo and espresso-streamers is caught mechanically: the fixture
suite encodes a real `EspressoBatch` with the pinned Go module, and CI regenerates
the fixtures and fails on any diff. The upgrade ritual when the format does change:
bump the espresso-streamers pin in `tests/fixtures/gen/go.mod`, regenerate
(`go run . -out ../`), let the failing tests show what moved, update
`src/batch_shape.rs` to match, and ship the sidecar image in the same coordinated deploy
as the batcher and streamer.

The pin is fixed, so CI does not track upstream streamer releases. Batch-format drift
surfaces when you bump the pin, not before, which is the right moment since the sidecar
redeploys together with the streamer anyway.

## Test fixtures

The JSON files under `tests/fixtures/` are committed expected outputs produced by a small Go program that uses op-geth's `TransactionArgs` encoding. Re-run the generator only if the wire format changes:

```bash
cd tests/fixtures/gen
go mod tidy          # first time, or after go.mod changes
go run . -out ../
```

Commit the updated `.json` files alongside any code change that affects `TransactionArgs` serialisation.

## Configuration reference

| Variable              | Required | Default           | Description                                                         |
|-----------------------|----------|-------------------|---------------------------------------------------------------------|
| `AWS_KMS_KEY_ID`      | yes      | —                 | KMS key ID or ARN                                                   |
| `AWS_REGION`          | yes      | —                 | AWS region                                                          |
| `CHAIN_ID`            | yes      | —                 | EVM chain ID; requests with other IDs are rejected                  |
| `LISTEN_ADDR`         | no       | `127.0.0.1:8547`  | TCP socket the JSON-RPC server binds to                             |
| `AWS_ENDPOINT_URL`    | no       | —                 | Override KMS endpoint (localstack)                                  |
| `ALLOWED_TO`          | no       | —                 | Comma-separated allowlist of `to` addresses                         |
| `TLS_CERT_FILE`       | no       | —                 | Path to PEM server certificate chain; enables TLS                   |
| `TLS_KEY_FILE`        | no       | —                 | Path to PEM server private key; required when TLS_CERT_FILE is set  |
| `TLS_CLIENT_CA_FILE`  | no       | —                 | Path to PEM CA certificate for verifying client certs; enables mTLS |
