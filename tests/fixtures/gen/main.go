// Generates (TransactionArgs JSON, expected signed RLP) fixture pairs using
// op-geth's TransactionArgs and a known private key. Run with:
//
//	go run . -out ../
//
// The output files are committed; re-run only when TransactionArgs encoding changes.
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
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	uint256 "github.com/holiman/uint256"
)

// fixture mirrors the shape our Rust test reads.
type fixture struct {
	Description string          `json:"description"`
	PrivateKey  string          `json:"privateKey"`
	Input       transactionArgs `json:"input"`
	ExpectedRLP string          `json:"expectedRlp"`
}

// transactionArgs matches go-ethereum's internal/ethapi.TransactionArgs JSON shape.
// We redeclare it here to avoid importing the internal package.
type transactionArgs struct {
	From                 *common.Address  `json:"from,omitempty"`
	To                   *common.Address  `json:"to,omitempty"`
	Gas                  *hexutil.Uint64  `json:"gas,omitempty"`
	GasPrice             *hexutil.Big     `json:"gasPrice,omitempty"`
	MaxFeePerGas         *hexutil.Big     `json:"maxFeePerGas,omitempty"`
	MaxPriorityFeePerGas *hexutil.Big     `json:"maxPriorityFeePerGas,omitempty"`
	Value                *hexutil.Big     `json:"value,omitempty"`
	Nonce                *hexutil.Uint64  `json:"nonce,omitempty"`
	Data                 *hexutil.Bytes   `json:"data,omitempty"`
	ChainID              *hexutil.Big     `json:"chainId,omitempty"`
	BlobFeeCap           *hexutil.Big     `json:"blobFeeCap,omitempty"`
	BlobHashes           []common.Hash    `json:"blobHashes,omitempty"`
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
		{
			name: "eip1559_basic",
			f:    eip1559Basic(privKey, signer, from, to),
		},
		{
			name: "eip1559_with_data",
			f:    eip1559WithData(privKey, signer, from, to),
		},
		{
			name: "eip4844_basic",
			f:    eip4844Basic(privKey, signer, from, to),
		},
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
	gas := hexutil.Uint64(21000)
	nonce := hexutil.Uint64(0)
	tx := types.NewTx(&types.DynamicFeeTx{
		ChainID:   big.NewInt(chainID),
		Nonce:     0,
		GasTipCap: big.NewInt(1_000_000),
		GasFeeCap: big.NewInt(1_000_000_000),
		Gas:       21000,
		To:        &to,
		Value:     big.NewInt(1),
	})
	return makeFixture("EIP-1559 basic transfer", key, signer, tx, transactionArgs{
		From:                 &from,
		To:                   &to,
		Gas:                  &gas,
		MaxFeePerGas:         (*hexutil.Big)(big.NewInt(1_000_000_000)),
		MaxPriorityFeePerGas: (*hexutil.Big)(big.NewInt(1_000_000)),
		Value:                (*hexutil.Big)(big.NewInt(1)),
		Nonce:                &nonce,
		ChainID:              (*hexutil.Big)(big.NewInt(chainID)),
	})
}

func eip1559WithData(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) fixture {
	gas := hexutil.Uint64(50000)
	nonce := hexutil.Uint64(1)
	data := hexutil.Bytes(common.FromHex("0xdeadbeef"))
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
	return makeFixture("EIP-1559 with calldata", key, signer, tx, transactionArgs{
		From:                 &from,
		To:                   &to,
		Gas:                  &gas,
		MaxFeePerGas:         (*hexutil.Big)(big.NewInt(2_000_000_000)),
		MaxPriorityFeePerGas: (*hexutil.Big)(big.NewInt(2_000_000)),
		Value:                (*hexutil.Big)(big.NewInt(0)),
		Nonce:                &nonce,
		Data:                 &data,
		ChainID:              (*hexutil.Big)(big.NewInt(chainID)),
	})
}

func eip4844Basic(key *ecdsa.PrivateKey, signer types.Signer, from, to common.Address) fixture {
	gas := hexutil.Uint64(21000)
	nonce := hexutil.Uint64(2)
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
	return makeFixture("EIP-4844 with blob hash", key, signer, tx, transactionArgs{
		From:                 &from,
		To:                   &to,
		Gas:                  &gas,
		MaxFeePerGas:         (*hexutil.Big)(big.NewInt(1_000_000_000)),
		MaxPriorityFeePerGas: (*hexutil.Big)(big.NewInt(1_000_000)),
		Value:                (*hexutil.Big)(big.NewInt(0)),
		Nonce:                &nonce,
		ChainID:              (*hexutil.Big)(big.NewInt(chainID)),
		BlobFeeCap:           (*hexutil.Big)(big.NewInt(100)),
		BlobHashes:           []common.Hash{blobHash},
	})
}

func makeFixture(desc string, key *ecdsa.PrivateKey, signer types.Signer, tx *types.Transaction, args transactionArgs) fixture {
	signed, err := types.SignTx(tx, signer, key)
	if err != nil {
		log.Fatalf("sign %s: %v", desc, err)
	}
	rlp, err := signed.MarshalBinary()
	if err != nil {
		log.Fatalf("encode %s: %v", desc, err)
	}
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
