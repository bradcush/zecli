# zecli

Command-line ZEC client

## Security

**NOT FOR PRODUCTION USE!** This code base uses
[zcash-devtool](https://github.com/zcash/zcash-devtool) as a starting point
which has not been written with security in mind. The eventual goal is to make
this particular client production ready, but for now use at your own risk. Some
but not all actions that are known ahead of time to compromise on security or
leak privacy will ask for further user confirmation before running.

## About

Spending some time learning Rust and Zcash, I need something to build. Best way
to focus on functionality is to build a command-line application. Rust is
relatively good for these. This code is heavily annotated with comments,
questions, and observations geared toward that goal.

zcash-devtool already implements a command-line tool but it's meant for
developers working w/ Zcash, not something that is supposed to be secure. My
goal is to understand how to build a light client by referencing this code base
but improve things to create something that is secure and has a good UX for
more typical Zcash users. I could have forked it but I want to reimplement
things to learn what's going on. Ideally security fixes and improvements will
be backported to the zcash-devtool as well.

Since we want to support what more typical users might need to interact with
Zcash, we remove certain functionality that's developer specific, most of
`inspect` which is mostly for debugging purposes.

## MVP

- [x] Support testnet interaction
- [x] Initialize a wallet and seed phrase
- [x] Set up some type of storage
- [ ] Sync with the current blockchain
- [x] View balance information

## Building

``` sh
cargo build
```

Check the release package:

``` sh
cargo check --release
```

## Running

``` sh
# Outputting wallet init help
env RUST_LOG=debug cargo run -- wallet init --help

# Or after building the binary
./target/debug/zecli wallet init --help
```

### Other examples

#### Initialize a testnet wallet

``` sh
cargo run --release -- wallet \
        --dir ../dev-wallet \
    init \
        --name "ZDevTest" \
        --identity ../dev-wallet/dev-key.txt \
        --network test \
        --server zecrocks
```

#### Retrieve wallet balance

``` sh
cargo run --release -- wallet \
    --dir ../dev-wallet balance
```

## Faucet

[zecfaucet.com](https://testnet.zecfaucet.com/)

## Resources

- [Zcash Documentation](https://zcash.readthedocs.io/en/latest/rtd_pages/testnet_guide.html)
- [ECC GitHub](https://github.com/Electric-Coin-Company)
- [Zcash GitHub](https://github.com/zash)
