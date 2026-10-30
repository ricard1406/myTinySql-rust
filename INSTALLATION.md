# Installation

How to install, build and run the Rust version of myTinySQL. It takes a few minutes on a Linux machine;
most of the time goes into installing Rust the first time.

## Requirements

| What | Needed | Notes |
| --- | --- | --- |
| Operating system | **Linux** | Tested on Ubuntu 26.04 LTS (x86-64). macOS and Windows should work too (see below), but have not been tested. |
| Rust | **1.88 or newer** (edition 2024) | Installs `rustc` (the compiler) and `cargo` (the build tool). Tested with Rust 1.99.1. |
| C linker | **`cc`** | Rust uses the system linker for the last step of the build. On Linux it comes with `build-essential` (gcc). |
| Libraries | **none to install** | Only Rust's standard library. Cargo downloads nothing: there are no dependencies. |
| Git | to download the code | Or download the zip from GitHub instead. |
| curl | to install Rust | The Rust installer is downloaded with it. |
| Web browser | optional | For the `--http` mode, e.g. Chromium or Firefox. |

**Other systems:** unlike the C++ version, which calls Linux socket functions directly (`MSG_NOSIGNAL`
exists only on Linux), this version uses only Rust's standard library (`std::net`, `std::fs`), which
works the same on Linux, macOS and Windows. So it should build and run there unchanged, but only Linux
has been tested.

## 1. Install the tools

**Ubuntu, Debian:**

```bash
sudo apt install build-essential curl git
```

**Fedora:**

```bash
sudo dnf install gcc curl git
```

Then install Rust with the official installer, [rustup](https://rustup.rs). It puts everything in your
home folder (`~/.rustup` and `~/.cargo`), with no `sudo`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Press Enter to accept the default installation. The installer adds `~/.cargo/bin` to your `PATH` for the
next terminals; to use Rust in the terminal you have open now, run:

```bash
. "$HOME/.cargo/env"
```

Check the version: it must say 1.88 or higher.

```bash
cargo --version
```

Your distribution may also have Rust packages (`sudo apt install rustc cargo`), but they are often older
than 1.88. If you use them, check the version the same way. With rustup, `rustup update` brings Rust up
to date at any time.

## 2. Get the code

```bash
git clone https://github.com/ricard1406/myTinySql-rust.git
cd myTinySql-rust
```

Without git: on the [GitHub page](https://github.com/ricard1406/myTinySql-rust) click **Code → Download ZIP**,
unzip it, and open a terminal in that folder.

## 3. Build

```bash
cargo build --release
```

This creates the program `./target/release/myTinySql`. Everything Cargo builds goes to `target/`.
Without `--release`, Cargo makes a debug build in `target/debug/`: it compiles faster, runs slower,
and stops with an error message on an integer overflow instead of wrapping around.

## 4. Check that it works

```bash
cargo test
```

The 9 unit tests and the 8 test scripts run (each script on its own copy of the sample tables).
Each test's line must end with `ok`, and every summary line must start with `test result: ok`.

Two more checks, which should report nothing:

```bash
cargo clippy --all-targets
```

```bash
cargo fmt --check
```

`cargo clippy` is Rust's linter, and `cargo fmt` its code formatter; rustup installs both.

## 5. Run it

Always run the program from the project folder: it reads its tables from `data/` in the current folder (or use `--data=DIR`).

**Run a SQL script:**

```bash
./target/release/myTinySql --sqlfile=examples/select.sql
```

**Type statements at a prompt** (Ctrl+D to quit):

```bash
./target/release/myTinySql
```

**Use it from a browser:**

```bash
./target/release/myTinySql --http
```

Then open http://localhost:8080. The page lists the tables on the left; you can search, add, edit and
delete rows, create new tables, and run any SQL in the console at the bottom. Stop the server with Ctrl+C.

All options:

```bash
./target/release/myTinySql --help
```

`cargo run --release -- <options>` builds (if needed) and runs in one step: for example
`cargo run --release -- --http`. The `--` separates Cargo's options from the program's.

## 6. Optional: the code documentation

```bash
cargo doc --no-deps --open
```

It builds the HTML documentation from the `///` comments in `src/` and opens it in the browser
(the files are in `target/doc/mytinysql/`). `--no-deps` leaves out the documentation of Rust's own
standard library, which is online at https://doc.rust-lang.org/std/.

## Troubleshooting

| Problem | Cause and fix |
| --- | --- |
| `cargo: command not found` | Rust is installed, but `~/.cargo/bin` is not in this terminal's `PATH`. Run `. "$HOME/.cargo/env"`, or open a new terminal. If it happens in every new terminal, add that line to the end of `~/.bashrc`. |
| ``error: linker `cc` not found`` while building | There is no C linker. Install `build-essential` (Ubuntu, Debian) or `gcc` (Fedora), then build again. |
| `error: rustc 1.80.0 is not supported by the following packages:` ... `mytinysql@0.1.0 requires rustc 1.88` | Rust is too old. Run `rustup update`, or, with your distribution's packages, install Rust with rustup instead. |
| ``data directory 'data' not found`` | The program was started outside the project folder. `cd` into it, or pass `--data=/path/to/myTinySql-rust/data`. |
| ``web page 'web/index.html' not found`` | The program was started outside the project folder, so the page's files in `web/` were not found. `cd` into it, or pass `--web=/path/to/myTinySql-rust/web`. |
| `bind to port 8080: Address already in use (os error 98)` | Another program (or another myTinySql, perhaps the C++ version) already uses port 8080. Stop it, or choose another port: `./target/release/myTinySql --http --port=8081`. |
| The browser page does not load from another computer | The server listens on all network interfaces, but a firewall may block port 8080. Use `--allow=192.168.` to accept only your home network. |

## Uninstall

The program installs nothing outside the project folder: to remove it, delete the folder. Its `target/`
subfolder holds all the build output; `cargo clean` removes just that.

To remove Rust itself (only if nothing else on the computer needs it):

```bash
rustup self uninstall
```
