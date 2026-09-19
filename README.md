## setup
needs esp-rs toolchain

1. install rustup
2. install `espup` package: `cargo install espup`
3. run `espup install`
4. source $HOME/export-esp.sh
5. install `ldproxy` and `espflash` cargo packages

## building:
    `cargo build`
## flash + monitor (over usb):
    `cargo run`