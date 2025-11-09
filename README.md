# zecli

Command-line ZEC client

## Security

**NOT FOR PRODUCTION USE!** This code base uses
[zcash-devtool](https://github.com/zcash/zcash-devtool) as a starting point
which has not been written with security in mind. The eventual goal is to make
this production ready, but for now use at your own risk.

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

## MVP

- [x] Support testnet interaction
- [x] Initialize a wallet and seed phrase
- [x] Set up some type of storage
- [ ] Sync with the current blockchain
- [ ] View balance information

## Building

``` sh
cargo build
```

## Running

``` sh
# Outputting wallet init help
env RUST_LOG=debug cargo run -- wallet init --help

# Or after building the binary
./target/debug/zecli wallet init --help
```

### Other examples

``` sh
# Initialize a testnet wallet
env RUST_LOG=debug cargo run -- wallet \
        --dir ../dev-wallet \
    init \
        --name "ZDevTest" \
        --identity ../dev-wallet/dev-key.txt \
        --network test \
        --server zecrocks
```

## Faucet

[zecfaucet.com](https://testnet.zecfaucet.com/)

## Resources

- [Zcash Documentation](https://zcash.readthedocs.io/en/latest/rtd_pages/testnet_guide.html)
- [ECC GitHub](https://github.com/Electric-Coin-Company)
- [Zcash GitHub](https://github.com/zash)
