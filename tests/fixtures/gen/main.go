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

	opsigner "github.com/ethereum-optimism/optimism/op-service/signer"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
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
// on the wire from op-batcher's signer client. See ethSignBasic for how it is
// produced; the goal is to never reconstruct the wire format by hand.
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
		"eth_sign_basic": ethSignBasic(privKey, from),
	}
	for name, f := range signFixtures {
		writeFixture(*outDir, name, f)
	}
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

// ethSignBasic produces the JSON-RPC params bytes that go-ethereum's rpc client
// places on the wire for `signerClient.Sign(ctx, addr, digest)` — see
// op-service/signer/espresso.go and rpc/client.go:newMessage, which both reduce
// to `json.Marshal(paramsIn)` on the variadic args.
func ethSignBasic(key *ecdsa.PrivateKey, from common.Address) signFixture {
	digest := crypto.Keccak256([]byte("espresso-batch-payload"))
	sig, err := crypto.Sign(digest, key)
	if err != nil {
		log.Fatalf("eth_sign_basic: %v", err)
	}
	params, err := json.Marshal([]any{from, digest})
	if err != nil {
		log.Fatalf("eth_sign_basic params: %v", err)
	}
	return signFixture{
		Description: "eth_sign of a keccak256 digest",
		PrivateKey:  testPrivKeyHex,
		Params:      params,
		ExpectedSig: "0x" + hex.EncodeToString(sig),
	}
}

func uint256From(v int64) *uint256.Int {
	return uint256.NewInt(uint64(v))
}
