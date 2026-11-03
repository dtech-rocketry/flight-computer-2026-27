## Setup

#### linux:

Run the following command to source the script from your home directory:

```bash
. $HOME/export-esp.sh
```

## Build & Deploy

#### linux:

run the following command to build the project into releases:

```bash
cargo build --release
```

run the following command to run the release on the board
```bash
cargo run --release
```

run the following command to flash the release onto the board:

```bash
espflash flash --monitor target/xtensa-esp32s3-none-elf/release/flight-computer-2026-27
```
