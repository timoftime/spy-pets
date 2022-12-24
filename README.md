# <h1 align="center">SpyPETs</h1>
This is a prototype of universal covert privacy-enhancing transactions (PETs) inspired by Gregory Maxwell's [CoinSwap](https://gist.github.com/chris-belcher/9144bd57a91c194e332fb5ca371d0964) design and Universal Atomic Swaps [\[TMM'21\]](https://eprint.iacr.org/2021/1612).

## Usage

### Setup wallet

First let's setup source wallet. You can generate new, recover from hex or from BIP39 mnemonic. Run following command:

```bash
cargo run -- setup
```

### Run market-maker daemon

Bob will be market maker. He runs daemon with configured funded wallet and specifies target address where ETH will be transferred after swap.

**Usage:** &nbsp; `cargo run -- transfer [OPTIONS] -w <wallet> -a <server-address> -s <secondary-address>`

#### Example:
```bash
cargo run -- provide -w bob -p -t 3C44CdDdB6a900fa2b585dd299e03d12FA4293BC
```

### Covert transfer

Alice will be market taker. She knows the URL of the Bob's daemon and runs client with specified target address where ETH will be transferred after swap and the amount of ETH she wishes to swap.

**Usage:** &nbsp; `cargo run -- transfer [OPTIONS] -w <wallet> -a <relay-address> [TARGET-ADDRESS] [AMOUNT]`

#### Example:
```bash
cargo run -- transfer -w alice -a http://127.0.0.1:8000 90F79bf6EB2c4f870365E785982E1f101E93b906 1.0
```

### Delayed withdrawals

Alice can ask Bob to withdraw after a certain delay using `--withdraw-delay` (`-d`) option.
This will make time/amount correlation attacks harder, as now Alice's and Bob's withdraw transactions are spread over multiple arbitrary distant blocks.

Alice can enforce this delay by using Verifiable Timed Commitments (VTC) in the last rounds of the SpyPets protocol.

#### Example:
```bash
cargo run -- transfer -w alice -a http://127.0.0.1:8000 -d 45s 90F79bf6EB2c4f870365E785982E1f101E93b906 1.0
```

### Swap ETH/ERC20 on Uniswap

**Usage:** &nbsp; `cargo run -- uniswap [OPTIONS] -w <wallet> -a <relay-address> [TARGET-ADDRESS] [AMOUNT] [ERC20]`

#### Example:
```bash
cargo run -- uniswap -w alice -a http://127.0.0.1:8000 90F79bf6EB2c4f870365E785982E1f101E93b906 1.0 USDC
```

### Purchase NFT from [ChainSafe Marketplace](https://marketplace.chainsafe.io/)

**Usage:** &nbsp; `cargo run -- buy-nft [OPTIONS] -w <wallet> -a <relay-address> -c <nft-contract> -i <token-id> [TARGET-ADDRESS] [PRICE]`

#### Example:
```bash
cargo run -- buy-nft -w alice -a http://127.0.0.1:8000 --nft-contract 0x2c1867bc3026178a47a677513746dcc6822a137a --token-id 01559ae4021a392a727d4f5619b1689c29b1a951a4e5057f24064001 90F79bf6EB2c4f870365E785982E1f101E93b906 0.2
```

## Known issues
- Time-locked locked commitments aren't verifiable
