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

The signer logs the derived Ethereum address at startup. Configure the OP batcher:

```
--signer.endpoint http://127.0.0.1:8547
--signer.address  <logged address>
```

## Running in production (real KMS)

The binary reads standard AWS credential environment variables / instance profile.
Set only:

```
AWS_KMS_KEY_ID=<arn-or-key-id>
AWS_REGION=us-east-2
CHAIN_ID=<your-chain-id>
LISTEN_ADDR=127.0.0.1:8547        # localhost-only; cross-host deployments are not yet supported
```

Optionally restrict signing to a single BatchInbox address:

```
ALLOWED_TO=0xYourBatchInboxAddress
```

## Manual RPC calls

With the signer running, you can call it directly with `curl`:

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
      "from":                 "0x<address-from-startup-log>",
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

# Sign a 32-byte digest (op-batcher's Espresso batch-auth path via `eth_sign`).
# `data` must decode to exactly 32 bytes; accepted as base64 (op-batcher's
# wire format — Go's default JSON encoding of `[]byte`) or `0x`-hex (curl).
curl -s -X POST http://127.0.0.1:8547 \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_sign",
    "params": [
      "0x<address-from-startup-log>",
      "0x0000000000000000000000000000000000000000000000000000000000000001"
    ],
    "id": 1
  }'
```

`eth_signTransaction` returns a `0x`-prefixed RLP-encoded signed transaction ready to broadcast;
`chainId` must match `CHAIN_ID` (`0xAA36A7` = 11155111 for Sepolia). `eth_sign` returns a 65-byte
hex signature over the supplied digest (`r||s||v` with `v ∈ {0,1}`), with no Ethereum message prefix.

## Test fixtures

The JSON files under `tests/fixtures/` are committed expected outputs produced by a small Go program that uses op-geth's `TransactionArgs` encoding. Re-run the generator only if the wire format changes:

```bash
cd tests/fixtures/gen
go mod tidy          # first time, or after go.mod changes
go run . -out ../
```

Commit the updated `.json` files alongside any code change that affects `TransactionArgs` serialisation.

## Configuration reference

| Variable          | Required | Default           | Description                                  |
|-------------------|----------|-------------------|----------------------------------------------|
| `AWS_KMS_KEY_ID`  | yes      | —                 | KMS key ID or ARN                            |
| `AWS_REGION`      | yes      | —                 | AWS region                                   |
| `CHAIN_ID`        | yes      | —                 | EVM chain ID; requests with other IDs are rejected |
| `LISTEN_ADDR`     | no       | `127.0.0.1:8547` | TCP socket the JSON-RPC server binds to     |
| `AWS_ENDPOINT_URL`    | no       | —                 | Override KMS endpoint (localstack)                        |
| `ALLOWED_TO`          | no       | —                 | Comma-separated allowlist of `to` addresses               |
| `TLS_CERT_FILE`       | no       | —                 | Path to PEM server certificate chain; enables TLS         |
| `TLS_KEY_FILE`        | no       | —                 | Path to PEM server private key; required when TLS_CERT_FILE is set |
| `TLS_CLIENT_CA_FILE`  | no       | —                 | Path to PEM CA certificate for verifying client certs; enables mTLS |

