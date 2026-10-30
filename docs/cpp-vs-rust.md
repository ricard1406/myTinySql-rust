# myTinySQL: C++ vs Rust

The same program written twice: the C++20 version ([github.com/ricard1406/myTinySql](https://github.com/ricard1406/myTinySql),
v0.2.0, commit `ad6721d`) and this Rust port. These notes compare the two, idea by idea, with code from both.

The port was made on 2026-10-30 with Rust 1.99.1 (edition 2024). It keeps the C++ design: the same
modules with the same jobs, the same functions under Rust names (`parseStatement` → `parse_statement`),
the same error messages, and the same files on disk. Only the language changes.

## How we know they behave the same

| Check | Result |
| --- | --- |
| The 8 test scripts of `tests/`, with the C++ `.expected` files copied unchanged | All 8 identical, byte for byte (`cargo test`) |
| The 4 scripts of `examples/`, plus the data files they write | Identical output, identical data files |
| A script of 35 edge cases (overflows, `A0` widths, `!`, `@`, accents in `LIKE`, `''` quotes...) | Identical, except one C++ bug the port found (section 9), now fixed in C++ too |
| Every command-line option, its errors and exit codes, and standard input | Identical |
| Both servers side by side: page files, 404 and 405, `..` in the path, `/api/tables`, 7 kinds of `/api/sql` | Identical status, content type and body |
| An idle "preconnect" connection, and 20 INSERTs sent at once | Both: no waiting; 28 rows, no duplicate ID, every row 250 characters |

## At a glance

| | C++ | Rust |
| --- | --- | --- |
| Files | 8 `.cpp` + 7 `.hpp` | 10 `.rs` (no headers) |
| Lines (code / comments / blank) | 1,831 / 505 / 310 | 1,715 / 521 / 211 (including 9 unit tests) |
| Libraries | Standard library + POSIX sockets | Standard library only |
| Build | `Makefile` (39 lines), `make` | `Cargo.toml` (21 lines), `cargo build --release` |
| Errors | Exceptions (`throw SqlError`) | Return values (`Result<T, SqlError>`, `?`) |
| "Maybe a value" | `std::optional` | `Option` |
| A statement | `std::variant` + `std::visit` | `enum` + `match` |
| Tests | `tests/run_tests.sh` (bash, `diff`) | `cargo test`: 9 unit tests + the 8 golden scripts |
| Docs | Doxygen (`/// @brief`, `@param`) | rustdoc (`///` in Markdown), `cargo doc` |
| Memory safety | Checked at run time with sanitizers (TESTING.md check 8) | Checked at compile time; the project has no `unsafe` |
| Binary size (release) | 377 KB (+ libstdc++, linked dynamically) | 731 KB (Rust's std is linked in) |

## File by file

| C++ | Rust | Lines C++ → Rust | Notes |
| --- | --- | --- | --- |
| `Util.hpp` + `Util.cpp` | `util.rs` | 158 → 147 | `SqlError` becomes a plain struct with `Display`; unit tests at the bottom |
| `Statement.hpp` | `statement.rs` | 111 → 156 | Longer only because every enum variant has its own doc line |
| `Parser.cpp` | `parser.rs` | 555 → 562 | Same tokenizer and recursive descent; 4 unit tests |
| `Catalog.hpp` + `.cpp` | `catalog.rs` | 208 → 177 | |
| `TableFile.hpp` + `.cpp` | `table_file.rs` | 295 → 234 | Lines are bytes (`Vec<u8>`), see "Strings" |
| `Session.hpp` + `.cpp` | `session.rs` | 524 → 476 | `run()` overloads become `run_select()`, `run_insert()`... |
| `Output.hpp` + `.cpp` | `output.rs` | 186 → 169 | JSON still written by hand |
| `Server.hpp` + `.cpp` | `server.rs` | 400 → 287 | No socket class: `TcpListener` and `TcpStream` |
| `main.cpp` | `main.rs` + `lib.rs` | 209 → 239 | The interpreter is a library; `main.rs` is the command line around it |

## 1. No headers: modules

C++ splits each piece in two: the declarations in `include/Catalog.hpp`, the code in `src/Catalog.cpp`,
glued with `#include` and kept apart from other names with `namespace tinysql`. Rust has one file per
module. `src/lib.rs` lists them:

```rust
pub mod catalog;
pub mod output;
pub mod parser;
// ...
```

and a file uses another with `use`:

```rust
use crate::catalog::{CATALOG_TABLE, Catalog, Column, TableSchema};
```

What C++ hides in an anonymous `namespace { }` inside the `.cpp`, Rust hides by leaving out `pub`.
Everything is private unless marked `pub`, so `Parser`, `Token` and `tokenize()` are invisible outside
`parser.rs`, exactly as they are invisible outside `Parser.cpp`. There are no include guards
(`#pragma once`), no `-MMD` dependency files, and nothing can be declared in one place and defined
differently in another.

## 2. `std::variant` and `std::visit` → `enum` and `match`

The heart of both versions is the `Statement` type: one of eight kinds of statement.

C++ (`Statement.hpp`, `Session.cpp`):

```cpp
using Statement = std::variant<SelectStmt, InsertStmt, UpdateStmt, DeleteStmt, CreateTableStmt,
                               ColumnFormatStmt, DescribeStmt, CommentStmt>;

result = std::visit([this](const auto& stmt) { return run(stmt); }, statement);
```

Rust (`statement.rs`, `session.rs`):

```rust
pub enum Statement {
    Select(SelectStmt),
    Insert(InsertStmt),
    // ...
    Describe(DescribeStmt),
    Comment,
}

match statement {
    Statement::Select(stmt) => self.run_select(&stmt),
    Statement::Insert(stmt) => self.run_insert(&stmt),
    // ...
    Statement::Comment => Ok(StatementResult::silent()),
}
```

Both are checked by the compiler: forget a `run()` overload in C++ and `std::visit` does not compile;
forget an arm in Rust and the `match` does not compile ("non-exhaustive patterns"). The differences:

- Rust has no overloading, so the eight `run()` methods get eight names.
- A variant needs no struct of its own: `Comment` carries nothing, so C++'s empty `struct CommentStmt {}` disappears.
- `match` is part of the language and reads top to bottom; `std::visit` is a library template whose
  error messages, when something is wrong, can fill a screen.

## 3. Enums that carry data

A C++ `enum class` is only a list of names, so data that depends on the kind sits next to it.
`SelectItem` in C++:

```cpp
struct SelectItem {
    enum class Kind { Star, Column, Literal };
    Kind kind;
    std::string text;  ///< The column name or the literal text (`*` for Kind::Star).
};
```

`text` means three different things, and for `Star` nothing at all. In Rust each variant carries
exactly what it needs:

```rust
pub enum SelectItem {
    Star,
    Column(String),
    Literal(String),
}
```

The port uses the same idea in three more places:

- The parser's `Token` (C++: a `TokenKind` plus a `text`) is `Word(String)`, `Quoted(String)`, `Symbol(String)` or `End`.
- The WHERE operator, a string in C++ (`op == "<="`), is an enum `Operator { Equal, NotEqual, Less, ... Like }`.
  A typo such as `"=<"` compiles in C++ and never matches; `Operator::LesOrEqual` does not compile.
- `Statement` itself (section 2).

## 4. Exceptions → `Result` and `?`

C++ throws an `SqlError` anywhere and catches it once, in `Session::execute()`. Nothing in a function's
signature says that it can throw (only the `@throws` comment). Rust has no exceptions: a function that
can fail says so in its return type, `SqlResult<T>`, short for `Result<T, SqlError>`, which holds either
`Ok(value)` or `Err(error)`.

C++ (`Catalog.cpp`):

```cpp
const TableSchema& Catalog::table(std::string_view name) const
{
    auto it = std::ranges::find_if(tables_, [&](const TableSchema& t) { return equalsIgnoreCase(t.name, name); });
    if (it == tables_.end()) {
        throw SqlError(std::format("unknown table '{}'", name));
    }
    return *it;
}
```

Rust (`catalog.rs`):

```rust
pub fn table(&self, name: &str) -> SqlResult<&TableSchema> {
    self.tables
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| SqlError::new(format!("unknown table '{name}'")))
}
```

The caller decides what to do with the error. Most callers pass it on with `?`, which means "if this is
an `Err`, return it from my function now":

```rust
let catalog = Catalog::load(&self.data_dir)?;
let schema = catalog.table(&stmt.table)?;
```

That `?` is the Rust spelling of "let the exception fly", but it is visible at every place where an
error can leave a function. `Session::execute()` is where C++ catches; in Rust it is a `match`:

```rust
let mut result = match parse_statement(sql).and_then(|statement| self.run(statement)) {
    Ok(result) => result,
    Err(error) => StatementResult { failed: true, message: error.to_string(), ..Default::default() },
};
```

Two smaller consequences:

- The parser's `[[noreturn]] void fail(...)`, which always throws, becomes `fn fail<T>(&self, expected: &str) -> SqlResult<T>`,
  which always returns an `Err`. Callers write `return self.fail("a table name");`.
- C++ also catches `std::exception` as an "internal error" (for example, `std::filesystem::rename` failing).
  Rust's file functions return `io::Result`, which the port turns into an `SqlError` on the spot with
  `.map_err(...)`, so no error can be forgotten.

## 5. `std::optional` → `Option`

They are the same idea, but `Option` comes with many small methods, so most `if (!x) { ... }` checks
turn into one chain. C++ `parseInteger` and `TableSchema::hasAutoId`:

```cpp
auto value = parseInteger(width);
if (!value || *value < 1) {
    throw SqlError(...);
}

bool TableSchema::hasAutoId() const
{
    return !columns.empty() && equalsIgnoreCase(columns.front().name, "ID");
}
```

Rust:

```rust
let Some(width) = parse_integer(&width).filter(|n| *n >= 1).and_then(|n| usize::try_from(n).ok()) else {
    return Err(SqlError::new(...));
};

pub fn has_auto_id(&self) -> bool {
    self.columns.first().is_some_and(|c| c.name.eq_ignore_ascii_case("ID"))
}
```

`let ... else` (also used for `Token::Word(word) = self.peek()`) means "take the value out, or else
leave the function". And there is no `*value` that can be written by mistake on an empty optional
(undefined behaviour in C++): the value can only be reached through a check.

## 6. References the compiler checks: lifetimes

`TableFile` keeps a reference to its table's schema. In C++ the rule is a comment:

```cpp
/// @param schema The table's definition; it must outlive this object.
TableFile(const TableSchema& schema, std::filesystem::path path);
```

In Rust the rule is part of the type:

```rust
pub struct TableFile<'a> {
    schema: &'a TableSchema,
    path: PathBuf,
}
```

`'a` (a *lifetime*) says "a `TableFile` lives no longer than the schema it borrows". Break the rule and
the program does not compile. A small example: a function that makes a schema and returns a `TableFile`
pointing at it, which in C++ compiles and then reads freed memory:

```text
error[E0515]: cannot return value referencing local variable `schema`
 --> main.rs:5:5
  |
5 |     TableFile { schema: &schema }
  |     ^^^^^^^^^^^^^^^^^^^^-------^^
  |     |                   |
  |     |                   `schema` is borrowed here
  |     returns a value referencing data owned by the current function
```

Most of the time the lifetimes are inferred and never written: `Catalog::table()` returns a
`&TableSchema` borrowed from the catalogue, and the compiler makes sure the catalogue is still there.

The same trick turns another C++ comment into a rule. `Server.cpp` says of `runSql()`: "Call it only
while holding workMutex". In Rust, `run_sql()` asks for the lock's guard as a parameter, so it cannot be
called without holding the lock:

```rust
fn run_sql(config: &ServerConfig, sql: &str, _lock: &MutexGuard<'_, ()>) -> Vec<u8>
```

## 7. Borrowing: one writer or many readers

Rust's central rule: at any moment, a value has either one `&mut` (writable) reference or any number of
`&` (read-only) references, never both. It rules out a classic C++ bug, changing a vector while holding
a reference into it. When `Catalog::load()` reads a line for a table it has not seen yet, it appends the
table and then adds the column to it. In C++:

```cpp
auto table = std::ranges::find_if(catalog.tables_, ...);
if (table == catalog.tables_.end()) {
    catalog.tables_.push_back(TableSchema{...});
    table = std::prev(catalog.tables_.end());   // must take the iterator again: push_back may move everything
}
```

The C++ code is correct, but only because the iterator is taken again after `push_back`. Forget that
line and it still compiles, then writes into freed memory. In Rust, keeping a reference across `push`
does not compile, so the port works with an index:

```rust
let index = match tables.iter().position(|t| t.name.eq_ignore_ascii_case(&table_name)) {
    Some(index) => index,
    None => {
        tables.push(TableSchema { name: table_name, columns: Vec::new(), record_length: 0 });
        tables.len() - 1
    }
};
let table = &mut tables[index];
```

## 8. Strings: `std::string_view` → `&str`, and UTF-8

The pairs match well:

| C++ | Rust | |
| --- | --- | --- |
| `std::string` | `String` | owns its text |
| `std::string_view` | `&str` | borrows text; nothing copied |
| `std::vector<char>` / bytes | `Vec<u8>` / `&[u8]` | raw bytes |

`trim()` returns a `string_view` into its argument in C++, and a `&str` into its argument in Rust. The
Rust compiler also checks that the text outlives the view (a `string_view` of a temporary string is a
classic C++ bug).

The big difference: **a Rust `String` is always valid UTF-8**. C++ strings are bytes, and myTinySQL
counts columns in bytes (`Città` takes 6 of a column's characters). Cutting a `&str` in the middle of a
character, as in `&line[60..65]`, stops the program with a panic. So the port:

- reads the data files as bytes (`fs::read` → `Vec<u8>`), cuts columns from the bytes, and only then
  turns each value into text (`String::from_utf8_lossy`, which shows invalid bytes as `�`);
- cuts SQL text only next to ASCII characters (spaces, commas, quotes), which never splits a character;
- uses `path.get(1..)` instead of `&path[1..]` where the text comes from outside (an HTTP request):
  `get` returns `None` instead of panicking;
- in `fit()` (console output), moves a cut back to the start of the character when it would split one.

One more surprise: Rust's own `str::trim()` removes every Unicode space (such as the no-break space
U+00A0); C++ `trim()` removes only space, tab, CR and LF. The port spells the four characters out to
keep the C++ behaviour.

## 9. Integers: overflow is caught

`std::size_t` arithmetic in C++ wraps around silently. In Rust, an overflow stops a debug build with a
panic (and wraps in a release build, unless asked otherwise), and conversions say what to do with values
that do not fit: `usize::try_from(n)` fails for a negative `n`.

The port found a real C++ bug this way (v0.2.0). `CREATE TABLE` adds up the column widths:

```sql
CREATE TABLE T1 (A CHAR(9223372036854775807), B CHAR(9223372036854775807), C CHAR(5))
```

```text
C++:   ERROR: value '9223372036854775806' is too long for column TO (max 5 characters)
Rust:  ERROR: a row of table T1 would be 18446744073709551615 characters long: the limit is 99999
```

In C++ the sum wraps around to 3, passes the "at most 99999 characters" check, and fails later with a
message that makes no sense. Rust adds with `saturating_add`, which stops at the largest number, so the
check catches it. Nothing is written in either version.

The C++ version has since been fixed the same way: before each addition it checks
`definition.width > kLargest - schema.recordLength` and stops at the largest `std::size_t`. Rust has
the same in one call, `saturating_add`, and also `checked_add` (gives `None` on overflow) and
`wrapping_add` (wraps on purpose). The case is now in `tests/create.sql` in both versions.

## 10. `std::function` → closures with `impl Fn`

`TableFile::removeRows()` takes "a test for each row". In C++ that is a `std::function`, which can hold
any callable but allocates and calls through a pointer:

```cpp
std::size_t removeRows(const std::function<bool(const Row&)>& shouldRemove) const;
```

In Rust, `impl Fn(&Row) -> bool` means "any closure with this shape". The compiler makes a copy of the
function for each closure passed in, so the call is direct (as with a C++ template):

```rust
pub fn remove_rows(&self, should_remove: impl Fn(&Row) -> bool) -> SqlResult<usize>
```

Closures look alike: `[&](const Row& row) { return matches(row[*whereIndex], stmt.where); }` becomes
`|row| matches(&row[where_index], &stmt.condition)`. Rust has no capture list; the compiler works out
what to borrow, and `move` (section 12) says "take ownership instead".

There are also three kinds: `Fn` (only reads what it captured), `FnMut` (may change it) and `FnOnce`
(may consume it). `rewrite()` takes an `FnMut`, because its callback edits each line.

## 11. RAII without writing a class

The C++ server wraps each socket descriptor in a 30-line `Socket` class: a destructor that calls
`close()`, a deleted copy constructor (two copies would close twice), and a move constructor that
empties the source. In Rust, all of that is what `TcpStream` already does: it closes in its `drop`,
it cannot be copied, and moving is the default. When `handle_client(config, stream, ...)` returns, the
stream it took is dropped and the connection closed.

The difference in moves: after `std::move(client)` in C++, `client` is still there, empty, and using it
compiles. After a move in Rust, the old name cannot be used at all; the compiler says "value used after move".

## 12. Threads and shared state

Each connection gets its own thread in both versions.

C++:

```cpp
std::thread([config, ip, socket = std::move(client)] {
    handleClient(config, socket.fd(), ip);
}).detach();
```

Rust:

```rust
let config = Arc::clone(&config);
thread::spawn(move || handle_client(&config, stream, &ip));
```

- `move` hands the stream, the address and the settings to the thread.
- C++ copies the whole `ServerConfig` into every thread. Rust shares one copy through an `Arc` (atomic
  reference count, like `std::shared_ptr`), freed when the last thread is done.
- Dropping the thread's handle is the same as `detach()`.

What C++ cannot check: the compiler does not know whether something is safe to share between threads.
Rust does (the `Send` and `Sync` traits). Hand a thread a plain `Rc` (the non-atomic `shared_ptr`) or a
`&mut` to something another thread also uses, and it does not compile. Data races are compile errors.

The lock that keeps SQL one request at a time:

```cpp
std::mutex workMutex;
const std::scoped_lock lock(workMutex);
```

```rust
static WORK: Mutex<()> = Mutex::new(());
let lock = lock_work();
```

A Rust `Mutex` normally wraps the data it protects (`Mutex<Vec<Row>>`: there is no way to reach the
rows without locking). Here, as in C++, it protects files on disk, so it holds nothing: `()`.
If a thread panics while holding the lock, Rust marks it "poisoned" so that others know the data may be
half-changed. With no data, `lock_work()` takes it anyway.

Two system details the standard library handles:

- `MSG_NOSIGNAL`: writing to a closed connection sends SIGPIPE, which kills a program. C++ passes
  `MSG_NOSIGNAL` to `send()`. Rust programs ignore SIGPIPE from the start, so `write_all()` just returns an error.
- `SO_REUSEADDR`: C++ sets it with `setsockopt`; Rust's `TcpListener::bind` sets it by itself on Unix.

## 13. Formatting text

`std::format` (C++20) and `format!` look alike and are both checked at compile time. Rust can also
name a variable inside the braces:

```cpp
std::format("table {} has {} columns, got {} values", schema_.name, schema_.columns.size(), values.size())
```

```rust
format!("table {} has {columns} columns{hint}: got {} values", schema.name, values.len())
```

`format!` is a *macro* (the `!`): it is expanded at compile time, which is how it can read the text
between the quotes.

## 14. Build and tools

The C++ `Makefile` names the compiler, the flags, the header folder, the object files and the
dependency tracking. `Cargo.toml` names the package and the two targets (library and binary); Cargo
knows the rest. The commands:

| Task | C++ | Rust |
| --- | --- | --- |
| Build | `make` | `cargo build --release` |
| Run | `./myTinySql --http` | `cargo run --release -- --http`, or `./target/release/myTinySql --http` |
| Tests | `make test` | `cargo test` |
| Docs | `make doxygen` | `cargo doc --no-deps --open` |
| Warnings | `-Wall -Wextra -Wpedantic` | on by default, plus `cargo clippy` (a linter with 700+ checks) |
| Format | by hand | `cargo fmt` (settings in `rustfmt.toml`: lines up to 120 characters) |
| Clean | `make clean` | `cargo clean` |

A dependency (say, `serde_json` to write the JSON) would be one line under `[dependencies]`, downloaded
and built by Cargo. This port keeps the C++ choice: standard library only.

## 15. Tests

C++: a bash script runs each `tests/NAME.sql` and compares with `diff`. Rust has tests in the language:

- **Unit tests** sit at the bottom of the file they test, in a `#[cfg(test)] mod tests`, and can reach
  private functions. `parser.rs` tests `tokenize()` directly, which is impossible from outside in C++.
- **Integration tests** in `tests/` see only the public library. `tests/golden.rs` runs the same 8
  scripts as `run_tests.sh`, against the same `.expected` files. Cargo builds the binary first and tells
  the test where it is (`env!("CARGO_BIN_EXE_myTinySql")`).
- A small `macro_rules!` makes one `#[test]` per script, so `cargo test` lists each by name and runs
  them in parallel, each on its own copy of `tests/data/`.

`UPDATE_EXPECTED=1 cargo test --test golden` rewrites the `.expected` files, like `run_tests.sh --update`.

## 16. Documentation comments

Both use `///`. Doxygen has its own commands; rustdoc uses Markdown, and links by name:

```cpp
/// @brief Finds a table by name.
/// @param name The table name (case-insensitive).
/// @return The table's definition.
/// @throws SqlError If there is no such table.
```

```rust
/// Finds a table by name (case-insensitive); fails if there is no such table.
pub fn table(&self, name: &str) -> SqlResult<&TableSchema>
```

The parameter and return types already say much of what `@param`, `@return` and `@throws` say (the
`SqlResult` *is* the `@throws`), so Rust doc comments tend to be shorter. `#![warn(missing_docs)]` in
`lib.rs` warns about any public item without a comment, like Doxygen's "undocumented" warnings.
Both sites are published the same way, by a GitHub Actions workflow after every push to `main`:
[ricard1406.github.io/myTinySql](https://ricard1406.github.io/myTinySql/) (Doxygen) and
[ricard1406.github.io/myTinySql-rust](https://ricard1406.github.io/myTinySql-rust/) (rustdoc).

## 17. Memory safety: tested vs proven

TESTING.md check 8 builds the C++ version with AddressSanitizer and UndefinedBehaviorSanitizer and
runs the tests, to catch out-of-bounds reads, use-after-free and similar bugs *on the inputs tested*.
In Rust, safe code cannot have those bugs at all: references are checked by the borrow checker,
every `vec[i]` checks its bounds (an out-of-range index stops the program with a panic instead of
reading other memory), and there is no null pointer. The only way around it is an `unsafe` block, and
this project has none: `#![forbid(unsafe_code)]` at the top of `lib.rs` and `main.rs` makes the compiler refuse one. A sanitizer run is still useful for `unsafe` code; here there is nothing for it to find.

## Where the two versions differ (on purpose)

| Situation | C++ | Rust |
| --- | --- | --- |
| Bytes that are not valid UTF-8 in a script, a data file or a request | Passed through as they are | Shown as `�` |
| A display width that ends inside a 2-byte letter (`COLUMN CITTA FORMAT A5` on `Città`) | Prints half the letter | Stops before the letter, adds one space |
| Port already in use | `bind to port 8080: Address already in use` | `... Address already in use (os error 98)` |
| A request for a folder inside `web/` | `200` with an empty body | `404` |
| A request line with an empty path (`GET  HTTP/1.1`) | An exception in the thread (logged), no answer | `404` |

## Suggested reading order

Read each Rust file next to its C++ twin, in the order the data flows:

1. `util.rs` ↔ `Util.hpp`/`.cpp`: `SqlError`, `Result`, `Option`, and the first unit tests.
2. `statement.rs` ↔ `Statement.hpp`: enums with data.
3. `parser.rs` ↔ `Parser.cpp`: the biggest file; bytes vs `&str`, `let ... else`, `?`.
4. `catalog.rs` ↔ `Catalog.hpp`/`.cpp`: borrowing (section 7).
5. `table_file.rs` ↔ `TableFile.hpp`/`.cpp`: lifetimes and closures.
6. `session.rs` ↔ `Session.hpp`/`.cpp`: `match` on `Statement`, `..Default::default()`.
7. `output.rs` ↔ `Output.hpp`/`.cpp`: iterators (`map`, `zip`, `collect`, `join`).
8. `server.rs` ↔ `Server.hpp`/`.cpp`: threads, `Arc`, `Mutex`, RAII.
9. `main.rs` ↔ `main.cpp`: `ExitCode`, `&mut dyn Read`.
