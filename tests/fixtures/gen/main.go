// Generates (TransactionArgs JSON, expected signed RLP) fixture pairs by calling
// op-service/signer's NewTransactionArgsFromTransaction directly, so the wire
// shape stays pinned to whatever op-batcher actually sends. Run with:
//
//	go run . -out ../
//
// Pinned to upstream ethereum-optimism/optimism; valid as long as
// EspressoSystems/optimism-espresso-integration leaves op-service/signer
// untouched. If that ever changes, swap in a `replace` directive.
package main

import (
	"crypto/ecdsa"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"math/big"
	"os"
	"path/filepath"

	"github.com/EspressoSystems/espresso-streamers/op/derivation"
	"github.com/ethereum-optimism/optimism/op-node/rollup/derive"
	opsigner "github.com/ethereum-optimism/optimism/op-service/signer"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/rlp"
	uint256 "github.com/holiman/uint256"
)

// txFixture mirrors the shape the Rust tx-builder fixtures read.
type txFixture struct {
	Description string                    `json:"description"`
	PrivateKey  string                    `json:"privateKey"`
	Input       *opsigner.TransactionArgs `json:"input"`
	ExpectedRLP string                    `json:"expectedRlp"`
}

// signFixture stores the JSON-RPC `params` array verbatim as it would appear
// on the wire from op-batcher's signer client. See espressoSignBatchBasic for
// how it is produced; the goal is to never reconstruct the wire format by hand.
type signFixture struct {
	Description string          `json:"description"`
	PrivateKey  string          `json:"privateKey"`
	Params      json.RawMessage `json:"params"`
	ExpectedSig string          `json:"expectedSig"`
}

const (
	// Hardhat account #0 — a well-known test key, never use in production.
	testPrivKeyHex = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
	chainID        = 11155111 // Sepolia
)

func main() {
	outDir := flag.String("out", ".", "output directory for fixture JSON files")
	flag.Parse()

	privKey, err := crypto.HexToECDSA(testPrivKeyHex)
	if err != nil {
		log.Fatalf("invalid private key: %v", err)
	}
	from := crypto.PubkeyToAddress(privKey.PublicKey)
	to := common.HexToAddress("0xff00000000000000000000000000000011155111")
	signer := types.NewCancunSigner(big.NewInt(chainID))

	txFixtures := map[string]txFixture{
		"eip1559_basic":     eip1559Basic(privKey, signer, from, to),
		"eip1559_with_data": eip1559WithData(privKey, signer, from, to),
		"eip4844_basic":     eip4844Basic(privKey, signer, from, to),
	}
	for name, f := range txFixtures {
		writeFixture(*outDir, name, f)
	}

	signFixtures := map[string]signFixture{
		"espresso_sign_batch_basic": espressoSignBatchBasic(privKey, from),
	}
	for name, f := range signFixtures {
		writeFixture(*outDir, name, f)
	}

	writeFixture(*outDir, "espresso_sign_batch_rejects", espressoSignBatchRejects(from, to))
}

func writeFixture(outDir, name string, f any) {
	path := filepath.Join(outDir, name+".json")
	data, err := json.MarshalIndent(f, "", "  ")
	if err != nil {
		log.Fatalf("marshal %s: %v", name, err)
	}
	if err := os.WriteFile(path, data, 0644); err != nil {
		log.Fatalf("write %s: %v", path, err)
	}
	fmt.Printf("wrote %s\n", path)
}

func eip1559Basic(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) txFixture {
	tx := types.NewTx(&types.DynamicFeeTx{
		ChainID:   big.NewInt(chainID),
		Nonce:     0,
		GasTipCap: big.NewInt(1_000_000),
		GasFeeCap: big.NewInt(1_000_000_000),
		Gas:       21000,
		To:        &to,
		Value:     big.NewInt(1),
	})
	return makeFixture("EIP-1559 basic transfer", key, signer, tx, from)
}

func eip1559WithData(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) txFixture {
	tx := types.NewTx(&types.DynamicFeeTx{
		ChainID:   big.NewInt(chainID),
		Nonce:     1,
		GasTipCap: big.NewInt(2_000_000),
		GasFeeCap: big.NewInt(2_000_000_000),
		Gas:       50000,
		To:        &to,
		Value:     big.NewInt(0),
		Data:      common.FromHex("0xdeadbeef"),
	})
	return makeFixture("EIP-1559 with calldata", key, signer, tx, from)
}

func eip4844Basic(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) txFixture {
	blobHash := common.HexToHash("0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014")
	tx := types.NewTx(&types.BlobTx{
		ChainID:    uint256From(chainID),
		Nonce:      2,
		GasTipCap:  uint256From(1_000_000),
		GasFeeCap:  uint256From(1_000_000_000),
		Gas:        21000,
		To:         to,
		Value:      uint256From(0),
		BlobFeeCap: uint256From(100),
		BlobHashes: []common.Hash{blobHash},
	})
	return makeFixture("EIP-4844 with blob hash", key, signer, tx, from)
}

// makeFixture signs the transaction and builds the fixture input by calling the
// exact same constructor op-batcher uses on the wire.
func makeFixture(desc string, key *ecdsa.PrivateKey, signer types.Signer, tx *types.Transaction, from common.Address) txFixture {
	signed, err := types.SignTx(tx, signer, key)
	if err != nil {
		log.Fatalf("sign %s: %v", desc, err)
	}
	rlp, err := signed.MarshalBinary()
	if err != nil {
		log.Fatalf("encode %s: %v", desc, err)
	}
	args := opsigner.NewTransactionArgsFromTransaction(big.NewInt(chainID), &from, signed.WithoutBlobTxSidecar())
	return txFixture{
		Description: desc,
		PrivateKey:  testPrivKeyHex,
		Input:       args,
		ExpectedRLP: "0x" + hex.EncodeToString(rlp),
	}
}

// sampleEspressoBatch builds a batch from the real streamer type. The sidecar
// shape-checks but decodes no field, so the contents stay at zero values; what is
// load-bearing is the type itself (REQ-SIDECAR-S-003): if it gains or loses an RLP
// field, regenerating fixtures fails the Rust suite instead of production. Number
// and Difficulty are set because those header fields must be non-nil to RLP-encode.
func sampleEspressoBatch(from common.Address) derivation.EspressoBatch {
	return derivation.EspressoBatch{
		BatchHeader:   &types.Header{Number: big.NewInt(101), Difficulty: big.NewInt(0)},
		Batch:         derive.SingularBatch{},
		L1InfoDeposit: types.NewTx(&types.DepositTx{}),
		SignerAddress: from,
	}
}

// espressoSignBatchBasic mirrors the wire call the batcher makes for
// `espresso_signBatch`: params are (address, batch RLP), the RLP bytes base64
// encoded by Go's JSON marshaling of []byte, and the expected signature is
// go-ethereum's crypto.Sign over keccak256 of exactly those bytes — the digest
// espresso-streamers' ToEspressoTransaction computes before signing.
func espressoSignBatchBasic(key *ecdsa.PrivateKey, from common.Address) signFixture {
	payload, err := rlp.EncodeToBytes(sampleEspressoBatch(from))
	if err != nil {
		log.Fatalf("espresso_sign_batch_basic encode: %v", err)
	}
	sig, err := crypto.Sign(crypto.Keccak256(payload), key)
	if err != nil {
		log.Fatalf("espresso_sign_batch_basic sign: %v", err)
	}
	params, err := json.Marshal([]any{from, payload})
	if err != nil {
		log.Fatalf("espresso_sign_batch_basic params: %v", err)
	}
	return signFixture{
		Description: "espresso_signBatch of an RLP-encoded EspressoBatch",
		PrivateKey:  testPrivKeyHex,
		Params:      params,
		ExpectedSig: "0x" + hex.EncodeToString(sig),
	}
}

// rejectFixture is a payload espresso_signBatch must refuse before any KMS call
// (REQ-SIDECAR-S-003 AC-1). Payload marshals to base64, as on the wire.
type rejectFixture struct {
	Description string `json:"description"`
	Payload     []byte `json:"payload"`
}

// espressoSignBatchRejects covers the signing preimages the shape check exists
// to exclude — none of them is an RLP list of four elements whose first element
// is a list — plus structural near-misses around that rule.
func espressoSignBatchRejects(from, to common.Address) []rejectFixture {
	mustRLP := func(desc string, v any) []byte {
		b, err := rlp.EncodeToBytes(v)
		if err != nil {
			log.Fatalf("rejects %s: %v", desc, err)
		}
		return b
	}
	legacyPreimage := mustRLP("legacy", []any{
		uint64(1), big.NewInt(1_000_000_000), uint64(21_000), to, big.NewInt(1), []byte{},
		big.NewInt(chainID), uint(0), uint(0),
	})
	eip1559Preimage := append([]byte{0x02}, mustRLP("eip1559", []any{
		big.NewInt(chainID), uint64(1), big.NewInt(1_000_000), big.NewInt(1_000_000_000),
		uint64(21_000), to, big.NewInt(1), []byte{}, []any{},
	})...)
	eip4844Preimage := append([]byte{0x03}, mustRLP("eip4844", []any{
		big.NewInt(chainID), uint64(2), big.NewInt(1_000_000), big.NewInt(1_000_000_000),
		uint64(21_000), to, big.NewInt(0), []byte{}, []any{},
		big.NewInt(100), []common.Hash{common.HexToHash("0x01")},
	})...)
	validBatch := mustRLP("valid batch", sampleEspressoBatch(from))
	return []rejectFixture{
		{"legacy transaction signing preimage: nine-element list, first element a scalar", legacyPreimage},
		{"EIP-1559 transaction signing preimage: type byte prefix, not an RLP list", eip1559Preimage},
		{"EIP-4844 transaction signing preimage: type byte prefix, not an RLP list", eip4844Preimage},
		{"EIP-191 signed message: not RLP at all", []byte("\x19Ethereum Signed Message:\n5hello")},
		{"EIP-712 signing preimage: 0x19 0x01 prefix, not RLP", append([]byte{0x19, 0x01}, make([]byte, 64)...)},
		{"three-element list", mustRLP("three elements", []any{[]any{}, []byte{}, []byte{}})},
		{"four-element list whose first element is not a list", mustRLP("first not a list", []any{[]byte{0x01}, []any{}, []byte{}, []byte{}})},
		{"five-element list whose first element is a list", mustRLP("five elements", []any{[]any{}, []byte{}, []byte{}, []byte{}, []byte{}})},
		{"valid batch followed by a trailing byte", append(append([]byte{}, validBatch...), 0x00)},
		{"empty payload", []byte{}},
	}
}

func uint256From(v int64) *uint256.Int {
	return uint256.NewInt(uint64(v))
}
