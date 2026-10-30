# myTinySQL in Rust

A tiny SQL interpreter over fixed-width text files, written in Rust as a learning project.

It is a port of the C++20 version, [github.com/ricard1406/myTinySql](https://github.com/ricard1406/myTinySql) (v0.2.0):
same statements, same files, same browser page, same output. Every C++ file has a Rust module with the same job,
so the two can be read side by side. **[docs/cpp-vs-rust.md](docs/cpp-vs-rust.md)** compares them, idea by idea.

**License:** [MIT](LICENSE). The software is provided "as is", without any warranty, and the author accepts no
responsibility for its use: see the last section, "License and responsibility".

> **DISCLAIMER: All data in this project is sample data, for testing purposes only.
> The companies, addresses and other values are fictional or AI generated. Many names in the
> `EMPLOY` table are those of historical Italian composers, used only as sample values;
> the other values in those rows (such as city and period) are sample data and may not be accurate. Any other resemblance
> to real persons, living or dead, or actual places is purely coincidental.**

## Install Rust

The build needs Rust 1.88 or newer (edition 2024). The official installer puts it in `~/.cargo`, with no `sudo`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then open a new terminal, or run `. "$HOME/.cargo/env"`, so that `cargo` is found. There is nothing else to
install: the project uses only Rust's standard library, so Cargo downloads nothing.

## Build and run

```bash
cargo build --release                                     # builds ./target/release/myTinySql
cargo test                                                # 9 unit tests + the 8 golden scripts of tests/
cargo doc --no-deps --open                                # HTML docs of the code, in the browser
./target/release/myTinySql --sqlfile=examples/select.sql  # run a script
./target/release/myTinySql                                # type statements interactively
./target/release/myTinySql --http                         # browser UI on http://localhost:8080
./target/release/myTinySql --help                         # all options
```

`cargo run --release -- <options>` builds and runs in one step, e.g. `cargo run --release -- --http`.

Supported statements:

```sql
SELECT ID,NOME,'lives in',CITTA FROM EMPLOY WHERE CITTA = Torino
SELECT * FROM CAL_WEEKLY WHERE CALORIES >= 15000
SELECT * FROM EMPLOY WHERE NOME LIKE '%fran%'
INSERT INTO EMPLOY VALUES (Mario,Rossi,Via Garibaldi,Bologna,Moderno)
UPDATE EMPLOY SET CITTA = 'Reggio Emilia' WHERE ID = 2
DELETE FROM EMPLOY WHERE ID = 69
CREATE TABLE CARS (ID NUMBER(10), MODEL CHAR(20), YEAR NUMBER(4))
COLUMN NOME FORMAT A12
DESCRIBE
DESCRIBE EMPLOY
REM a comment
```

The full description of the statements, the file formats and the web page is in the C++ project's
[project guide](https://github.com/ricard1406/myTinySql/blob/main/docs/project-guide.md): all of it applies here.

## Layout

| Path | What |
| --- | --- |
| `src/` | The interpreter: `lib.rs` lists the modules, `main.rs` is the command line |
| `tests/golden.rs` | Runs every `tests/*.sql` and compares with `tests/*.expected` (copied unchanged from the C++ version) |
| `tests/data/` | The frozen tables the tests run on |
| `data/` | The database: `SYSTEM_TABLE` and one `<TABLE>_DATA` file per table |
| `examples/` | Sample scripts (`insert.sql` adds rows to `data/`) |
| `web/` | The browser page, the same files as in the C++ version |
| `docs/cpp-vs-rust.md` | The comparison of the two versions |
| `Cargo.toml`, `rustfmt.toml` | Build settings; formatting (`cargo fmt`) |

| C++ | Rust |
| --- | --- |
| `include/Util.hpp`, `src/Util.cpp` | `src/util.rs` |
| `include/Statement.hpp` | `src/statement.rs` |
| `src/Parser.cpp` | `src/parser.rs` |
| `include/Catalog.hpp`, `src/Catalog.cpp` | `src/catalog.rs` |
| `include/TableFile.hpp`, `src/TableFile.cpp` | `src/table_file.rs` |
| `include/Session.hpp`, `src/Session.cpp` | `src/session.rs` |
| `include/Output.hpp`, `src/Output.cpp` | `src/output.rs` |
| `include/Server.hpp`, `src/Server.cpp` | `src/server.rs` |
| `src/main.cpp` | `src/main.rs` (+ `src/lib.rs`) |

## Tests

`cargo test` runs the unit tests inside the modules (`util`, `parser`, `output`) and `tests/golden.rs`, which
runs each of the 8 test scripts on a fresh copy of `tests/data/`. The output must match the C++ version's
`.expected` files byte for byte. If you change the output on purpose:

```bash
UPDATE_EXPECTED=1 cargo test --test golden
```

then check the diff of the `.expected` files.

`cargo clippy` (Rust's linter) and `cargo fmt --check` should both report nothing.

## License and responsibility

myTinySQL is free software under the [MIT License](LICENSE): you may use, copy, modify and
share it, also for commercial purposes, as long as the copyright notice and the license text stay with it.

**No responsibility:** myTinySQL is a learning project. It is provided "as is", without warranty of any
kind, express or implied. The author accepts no responsibility for any damage, loss of data or other
consequence of using it, or of being unable to use it. Use it at your own risk, and keep a copy of any data
you care about.
