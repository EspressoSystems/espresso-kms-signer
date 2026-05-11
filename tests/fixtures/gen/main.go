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

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	opsigner "github.com/ethereum-optimism/optimism/op-service/signer"
	uint256 "github.com/holiman/uint256"
)

// fixture mirrors the shape our Rust test reads.
type fixture struct {
	Description string                  `json:"description"`
	PrivateKey  string                  `json:"privateKey"`
	Input       *opsigner.TransactionArgs `json:"input"`
	ExpectedRLP string                  `json:"expectedRlp"`
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

	fixtures := []struct {
		name string
		f    fixture
	}{
		{"eip1559_basic", eip1559Basic(privKey, signer, from, to)},
		{"eip1559_with_data", eip1559WithData(privKey, signer, from, to)},
		{"eip4844_basic", eip4844Basic(privKey, signer, from, to)},
	}

	for _, tc := range fixtures {
		path := filepath.Join(*outDir, tc.name+".json")
		data, err := json.MarshalIndent(tc.f, "", "  ")
		if err != nil {
			log.Fatalf("marshal %s: %v", tc.name, err)
		}
		if err := os.WriteFile(path, data, 0644); err != nil {
			log.Fatalf("write %s: %v", path, err)
		}
		fmt.Printf("wrote %s\n", path)
	}
}

func eip1559Basic(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) fixture {
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

func eip1559WithData(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) fixture {
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

func eip4844Basic(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) fixture {
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
func makeFixture(desc string, key *ecdsa.PrivateKey, signer types.Signer, tx *types.Transaction, from common.Address) fixture {
	signed, err := types.SignTx(tx, signer, key)
	if err != nil {
		log.Fatalf("sign %s: %v", desc, err)
	}
	rlp, err := signed.MarshalBinary()
	if err != nil {
		log.Fatalf("encode %s: %v", desc, err)
	}
	args := opsigner.NewTransactionArgsFromTransaction(big.NewInt(chainID), &from, signed.WithoutBlobTxSidecar())
	return fixture{
		Description: desc,
		PrivateKey:  testPrivKeyHex,
		Input:       args,
		ExpectedRLP: "0x" + hex.EncodeToString(rlp),
	}
}

func uint256From(v int64) *uint256.Int {
	return uint256.NewInt(uint64(v))
}
