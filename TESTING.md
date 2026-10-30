# Testing

This guide shows **why** the Rust version of myTinySQL works, not just that it does. Every check below is a
command you can copy, followed by the result you should see. If you get the same result, you have checked
that part yourself.

It follows the [TESTING.md of the C++ version](https://github.com/ricard1406/myTinySql/blob/main/TESTING.md):
the same checks, with the same numbers, adapted to Rust and Cargo, plus check 13, which compares the two
versions. These are the checks used while developing the port (on Ubuntu 26.04, with Rust 1.99.1), and every
command and expected result in this file was run before it was written down.

## All checks at once

One command runs checks 1 to 10, 12 and 13, compares every result with the one this guide expects, and ends
with a single verdict. First install the tools the checks use: Rust (see [INSTALLATION.md](INSTALLATION.md)),
and the ones below; **a fresh Ubuntu has no `curl`**, and without it the checks stop at once.

```bash
sudo apt install build-essential curl git
```

Then run all the checks:

```bash
tests/check_all.sh
```

**You should see** (output, don't type it) one line per check, then the verdict:

```text
myTinySQL in Rust: running the checks of TESTING.md (about half a minute)

  1  Clean build              ok
  2  Automated tests          ok   9 unit tests, 8 test scripts
  3  Command line             ok
  4  Browser server           ok
  5  Idle connection          ok   other request answered in 0.000710s
  6  Parallel writes          ok
  7  File integrity           ok   5 files
  8  Memory safety            ok
  9  Documentation            ok
 10  Fresh install            ok   the version on GitHub, 8 test scripts
 11  By hand, in a browser    --   by a person: see TESTING.md, check 11
 12  On GitHub                ok   CI of commit 99eba94
 13  Same as C++              ok   12 scripts, against ../myTinySQL/myTinySql

ALL DONE - IT WORKS   (12 checks passed)
```

If something is wrong, the verdict is `OOOPS - SOMETHING IS WRONG`, the failed checks say why, and the
script prints where its log is: everything each check printed, with the expected and actual results.
If a tool is missing, it says which one and how to install it, before running anything.

- `tests/check_all.sh --offline` skips the two checks that need the internet (10 and 12).
- Check 11 needs a person and a browser, so the script only lists it. Check 12 is skipped while the
  latest run on GitHub is still going.
- Check 13 needs the C++ program. The script looks for it in `../myTinySQL/myTinySql` (the C++ project
  next to this one, built with `make`); set `MYTINYSQL_CPP=/path/to/myTinySql` to use another one.
  Without it, check 13 is skipped.
- Like the checks below, it works on a temporary copy of the tables: `data/` is only read (checks 7 and 13).
- The C++ version runs its script with `make check`. Cargo has no such custom commands, so here it is
  `tests/check_all.sh` itself.

The rest of this guide is the same checks one by one, with what each one shows and why it exists.

## Summary

| # | Check | Command | Expected | What it shows |
| --- | --- | --- | --- | --- |
| 1 | Clean build | `cargo clean && cargo build --release`, `cargo clippy`, `cargo fmt --check` | no warnings | The compiler and the linter find nothing suspicious |
| 2 | Automated tests | `cargo test` | 9 + 8 × `ok` | The SQL gives exactly the expected output |
| 3 | Command line | `echo "..." \| ./target/release/myTinySql ...` | rows, errors, exit code | Scripts and error handling work |
| 4 | Browser server | `curl ...` | 200, 404, 405 | The server answers correctly |
| 5 | Idle connection | `exec 3<>/dev/tcp/...` | `200 OK` after 6 s | Browsers that "preconnect" are served |
| 6 | Parallel writes | 20 × `curl` at once | 28 rows, no duplicate ID | Simultaneous requests can't corrupt a table |
| 7 | File integrity | `awk` on `data/` | one length per file | Every row has the exact fixed width |
| 8 | Memory safety | `forbid(unsafe_code)`, debug build | 0 problems | No unsafe code; no overflow or out-of-bounds access |
| 9 | Documentation | `cargo doc --no-deps` | no warnings | Every public item is documented |
| 10 | Fresh install | `git clone` + `cargo test` | all `ok` | The project works from a clean copy |
| 11 | By hand, in a browser | checklist | as described | The web page works for a person |
| 12 | On GitHub | Actions tab | 2 green checks | The code on GitHub builds and passes its tests, and the online documentation is up to date |
| 13 | Same as C++ | both programs on every script | no difference | The port behaves exactly like the original |

## How to read this guide

Each check has two kinds of grey boxes:

- **Commands**: copy them into the terminal and press Enter.
- **Output**: every output box comes right after the words "**You should see**". It shows what the
  commands print. Don't type it: compare it with what your terminal shows. For example, a box containing
  only `0` means the command must print `0`.

## What you need

Rust and the build tools from [INSTALLATION.md](INSTALLATION.md), plus the tools some checks use. A fresh
Ubuntu has no `curl`, so install it even if you installed everything else. On Ubuntu:

```bash
sudo apt install build-essential curl git
```

| Tool | Used by | Note |
| --- | --- | --- |
| `cargo` | checks 1, 2, 8, 9, 10 | Comes with Rust (rustup also installs `cargo clippy` and `cargo fmt`) |
| `cc` | checks 1, 8, 10 | The C linker Rust uses; comes with `build-essential` |
| `curl` | checks 4, 5, 6, 12 | Sends requests to the server from the terminal; not installed on a fresh Ubuntu |
| `git` | check 10 | Downloads a fresh copy of the project |
| The C++ program | check 13 | Optional: built from [myTinySql](https://github.com/ricard1406/myTinySql) with `make` |
| A web browser | check 11 | E.g. Chromium or Firefox |

To see which tools are missing, run:

```bash
for tool in cargo cc curl git; do command -v $tool > /dev/null || echo "missing: $tool"; done; echo "tool check done"
```

**You should see** (output, don't type it) only `tool check done`. Each `missing:` line names a tool to
install: `cargo` with rustup (INSTALLATION.md, step 1), `cc` with `sudo apt install build-essential`,
the others with `sudo apt install` and their name. If only `cargo` is missing although Rust is installed,
run `. "$HOME/.cargo/env"` and try again.

## Before you start: work on a copy

Checks 3 to 6 change tables (they insert, update and delete rows). So that your real tables in `data/`
are never touched, they run on a copy of the frozen test tables in a temporary folder. Run these lines
once, in the project folder, in the same terminal you will use for the checks:

```bash
PROJECT=$(pwd)
TEST_DIR=$(mktemp -d)
cp -r tests/data "$TEST_DIR/data"
echo "TEST_DIR is: $TEST_DIR"
```

**You should see** (output, don't type it) a temporary folder; the letters after `tmp.` change every time:

```text
TEST_DIR is: /tmp/tmp.KDNZpJ10N7
```

`PROJECT` remembers the project folder, whatever its name; `TEST_DIR` is the temporary folder. Both exist
only in this terminal window: **run all the checks in the same window**. If you close it or open a new
one, run these lines again first.

Run each block below as it is, one line at a time or all at once. Keep each command on its own line:
a line ending with `&` runs in the background, and joining it to other commands with `&&` sends them all
to the background too.

## 1. Clean build

```bash
cargo clean && cargo build --release 2>&1 | grep -c '^warning'
```

**You should see** (output, don't type it):

```text
0
```

Then the linter and the formatter:

```bash
cargo clippy --all-targets 2>&1 | grep -c '^warning'
cargo fmt --check && echo "formatted"
```

**You should see** (output, don't type it):

```text
0
formatted
```

**Why it matters:** Rust's compiler warns by default about unused values, unreachable code and similar
mistakes (the C++ Makefile has to ask for them with `-Wall -Wextra -Wpedantic`). `cargo clippy` adds
hundreds of further checks, and `cargo fmt --check` makes sure the code has the standard layout (with the
settings in `rustfmt.toml`). All three stay at zero, so a new warning is noticed at once.

## 2. Automated tests

```bash
cargo test
```

**You should see** (output, don't type it; Cargo also prints `Compiling` lines, and the tests of each
group may finish in another order):

```text
     Running unittests src/lib.rs (target/debug/deps/mytinysql-b860d06986fcccc9)

running 9 tests
test output::tests::fit_pads_and_cuts ... ok
test output::tests::json_escapes ... ok
test parser::tests::parses_a_select ... ok
test parser::tests::apostrophes_stay_inside_words ... ok
test parser::tests::splits_at_semicolons_outside_quotes ... ok
test parser::tests::reports_what_it_expected ... ok
test util::tests::integers ... ok
test util::tests::like_patterns ... ok
test util::tests::trims_only_ascii_whitespace ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src/main.rs (target/debug/deps/myTinySql-bfed675f1df2d7ef)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/golden.rs (target/debug/deps/golden-b7b3135bed39a110)

running 8 tests
test create ... ok
test apostrophes ... ok
test describe ... ok
test delete ... ok
test errors ... ok
test insert ... ok
test update ... ok
test select ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
   Doc-tests mytinysql

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**How it works:** there are two kinds of tests.

- **Unit tests** (the 9 with `::` in their names) sit at the bottom of the file they test, in a
  `#[cfg(test)] mod tests`, and test single functions: `parser::tests::...` tests the tokenizer and the
  parser directly, without running the program.
- **The 8 test scripts** (`tests/golden.rs`): each `tests/NAME.sql` is a script of statements, and
  `tests/NAME.expected` is the exact output it must produce. These are the same files as in the C++ version.
  Each script runs on a fresh copy of `tests/data/`, and the output must match exactly. One different
  character is enough to fail. Together, they cover SELECT, WHERE with every operator and `LIKE`, INSERT,
  UPDATE, DELETE, CREATE TABLE, DESCRIBE, COLUMN, apostrophes in values (d'Asti, L'Aquila), and the error messages.

The two groups with 0 tests are normal: `src/main.rs` has no unit tests, and no doc comment has a code
example to run ("Doc-tests").

**If a test fails**, the line says `FAILED`, and below it the test prints the first line that differs,
with what was expected and what the program printed.

**Adding a test:** add statements to a `tests/*.sql` file, then run
`UPDATE_EXPECTED=1 cargo test --test golden` to record the output, and **read** the new `.expected` file
(or `git diff tests/`) to confirm the output is right before you commit it. For a new script, also add its
name to the `golden_tests!(...)` line at the end of `tests/golden.rs`; if you forget, `tests/check_all.sh`
notices, because the number of scripts and of tests differ.

**Why `tests/data/` and not `data/`:** the tests need tables that never change. If they used your working
tables, adding a row on the web page would make the tests fail although nothing is broken.

## 3. Command line

A search with `LIKE`:

```bash
echo "SELECT ID,NOME,COGNOME FROM EMPLOY WHERE NOME LIKE '%fran%'" | ./target/release/myTinySql --data="$TEST_DIR/data" --sqlfile=-
```

**You should see** (the first lines; output, don't type it):

```text
> SELECT ID,NOME,COGNOME FROM EMPLOY WHERE NOME LIKE '%fran%'
ID-------- NOME------------------------------------ COGNOME---------------------------------
11         Francesco                                Cavalli
16         Francesco                                Gasparini
17         Francesco                                Geminiani
```

A mistake, and the exit code a script returns when a statement fails:

```bash
echo "SELECT FOO FROM EMPLOY" | ./target/release/myTinySql --data="$TEST_DIR/data" --sqlfile=-
echo "exit code: $?"
```

**You should see** (output, don't type it):

```text
> SELECT FOO FROM EMPLOY
ERROR: unknown column 'FOO' in table EMPLOY

exit code: 1
```

**What it shows:** errors are reported clearly, the program does not stop, and a script that had a failing
statement ends with exit code 1, so other programs can detect it.

## 4. Browser server

First make sure that `curl` is installed, and that the copy of the tables from "Before you start" is there,
in this terminal window:

```bash
command -v curl > /dev/null && echo "curl found" || echo "curl is missing: sudo apt install curl"
test -d "$TEST_DIR/data" && echo "test copy found" || echo "TEST_DIR is empty: run the 'Before you start' lines in this window"
```

**You should see** (output, don't type it):

```text
curl found
test copy found
```

Start a test server in the background, on a spare port and on the copy of the tables:

```bash
./target/release/myTinySql --http --port=18080 --data="$TEST_DIR/data" > "$TEST_DIR/server.log" 2>&1 &
SERVER=$!
sleep 1
if kill -0 $SERVER 2>/dev/null; then echo "server is running"; else echo "server did not start:"; cat "$TEST_DIR/server.log"; fi
```

**You should see** (output, don't type it):

```text
server is running
```

If it says "server did not start", the line after it tells you why; see the note at the end of this check.

The terminal prints a line like `[1] 12345`: that is the background job's number and process ID, and it
is normal. When you stop the server at the end of check 6, it prints a line like `[1]+  Terminated` (in
Italian, `Terminato`): also normal.

Ask it for the page, a file that does not exist, a method it does not support, the list of tables,
a statement, and a file outside the page's folder:

```bash
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:18080/
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:18080/favicon.ico
curl -s -o /dev/null -w '%{http_code}\n' -X PUT http://localhost:18080/api/sql
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://localhost:18080/api/tables
curl -s --data-binary 'DESCRIBE' http://localhost:18080/api/sql | grep -o '"message":"[^"]*"'
curl -s -o /dev/null -w '%{http_code}\n' --path-as-is http://localhost:18080/../data/SYSTEM_TABLE
```

**You should see** (output, don't type it):

```text
200
404
405
200 application/json
"message":"4 tables"
404
```

`200` means OK, `404` "not found" and `405` "method not allowed". The page itself is `index.html` from the
`web/` folder; `/api/tables` and `/api/sql` are the API the page uses, and they answer in JSON. The last
line tries to read a file outside `web/` (`--path-as-is` makes `curl` send the `..` exactly as typed): the
server refuses, so nothing outside the page's folder can be read. Leave the server running for checks 5 and 6.

**If `curl` prints `000`:** it could not connect, so no server is running on port 18080. Usually the server
stopped right after starting. The most common reasons:

| Message in "server did not start" | Cause | Fix |
| --- | --- | --- |
| `bash: /server.log: Permission denied` | `TEST_DIR` is empty: the "Before you start" lines ran in another terminal window | Run them again in this window, then restart this check |
| `bind to port 18080: Address already in use (os error 98)` | Another server (perhaps from an earlier try, or the C++ version's checks) still uses the port | Stop it (`kill %1`, or close that terminal), then restart this check |

## 5. Idle connection ("preconnect")

Browsers such as Chromium open a connection early and may send nothing on it for a while. This opens such a
silent connection, checks that other requests are not blocked by it, then uses it after 6 seconds:

```bash
exec 3<>/dev/tcp/localhost/18080
curl -s -o /dev/null -w 'other request while idle: %{time_total}s\n' http://localhost:18080/
sleep 6
printf 'GET /api/tables HTTP/1.1\r\nHost: x\r\n\r\n' >&3
head -1 <&3
exec 3<&-
```

**You should see** (output, don't type it):

```text
other request while idle: 0.000500s
HTTP/1.1 200 OK
```

(The time will differ a little; it must be well under one second.)

**Why this check exists:** an earlier C++ version served one connection at a time, and in Chromium every
click then failed. Both versions now give each connection its own thread (in Rust, `thread::spawn` in
`src/server.rs`); this check makes sure the port kept that.

## 6. Parallel writes

Send 20 INSERTs at the same moment, and wait until all 20 are answered:

```bash
(
    for i in $(seq 1 20); do
        curl -s -o /dev/null --data-binary "INSERT INTO COMPANY VALUES ('Firm $i',Test,'Via $i',Roma)" http://localhost:18080/api/sql &
    done
    wait
)
```

The parentheses run the loop in a *subshell*, a separate copy of the shell. That does two things: the
terminal doesn't announce each of the 20 background requests, and `wait` waits only for those requests,
not for the server (which never ends).

Count the rows, the duplicate IDs, and the different row lengths:

```bash
wc -l < "$TEST_DIR/data/COMPANY_DATA"
cut -c1-10 "$TEST_DIR/data/COMPANY_DATA" | sort | uniq -d | wc -l
LC_ALL=C awk '{print length($0)}' "$TEST_DIR/data/COMPANY_DATA" | sort -u
```

**You should see** (output, don't type it):

```text
28
0
250
```

8 original rows + 20 new ones; no ID used twice; every row exactly 250 characters long. The server reads
requests in parallel, but runs the SQL one request at a time (the `static WORK: Mutex<()>` in
`src/server.rs`), so no two INSERTs can pick the same ID or write into each other.

Stop the test server:

```bash
kill $SERVER
```

## 7. File integrity

Every table file must have rows of one exact length (the one in `SYSTEM_TABLE`). This reads your real
tables, without changing them:

```bash
for f in data/*; do echo "$(basename $f): $(LC_ALL=C awk '{print length($0)}' "$f" | sort -u | tr '\n' ' ')"; done
```

**You should see** one length per file (output, don't type it):

```text
CAL_WEEKLY_DATA: 35
COMPANY_DATA: 250
EMPLOY_DATA: 290
SYSTEM_TABLE: 80
VINI-ITALY_DATA: 180
```

(Your list may differ if you created other tables: what matters is one length per file. `tests/check_all.sh`
also compares each length with the row length in `SYSTEM_TABLE`.)

`LC_ALL=C` makes `awk` count bytes, as the program does: an accented letter such as `ò` is 2 bytes.

## 8. Memory safety

The C++ version needs a special build with sanitizers to catch memory errors while the tests run. Rust
checks most of that when it compiles: the borrow checker refuses dangling references and data races,
and only code inside an `unsafe { }` block could get around it. This project has none, and both
`src/lib.rs` and `src/main.rs` start with `#![forbid(unsafe_code)]`, so the compiler refuses to build one:

```bash
grep -c '^#!\[forbid(unsafe_code)\]' src/lib.rs src/main.rs
```

**You should see** (output, don't type it):

```text
src/lib.rs:1
src/main.rs:1
```

What is left for run time are the checks Rust makes there: every `vec[i]` checks its bounds, and a debug
build also checks every integer operation for overflow. A failed check stops the program with a *panic*
message instead of reading the wrong memory or a wrong number. Build the debug version (it is in
`target/debug/`), then run every test script with it:

```bash
cargo build
for f in tests/*.sql; do
    rm -rf "$TEST_DIR/debug" && cp -r tests/data "$TEST_DIR/debug"
    target/debug/myTinySql --data="$TEST_DIR/debug" --sqlfile="$f" > /dev/null 2> "$TEST_DIR/debug.log"
    echo "$f: $(grep -c 'panicked' "$TEST_DIR/debug.log") problems"
done
```

**You should see** (output, don't type it, after Cargo's `Finished` line):

```text
tests/apostrophes.sql: 0 problems
tests/create.sql: 0 problems
tests/delete.sql: 0 problems
tests/describe.sql: 0 problems
tests/errors.sql: 0 problems
tests/insert.sql: 0 problems
tests/select.sql: 0 problems
tests/update.sql: 0 problems
```

(`cargo test` in check 2 also uses the debug build, so it makes the same checks; this loop shows them
one script at a time, like the sanitizer loop of the C++ version.)

## 9. Documentation coverage

```bash
cargo doc --no-deps 2>&1 | grep -c '^warning'
```

**You should see** (output, don't type it):

```text
0
```

`src/lib.rs` has `#![warn(missing_docs)]`, so `cargo doc` (and `cargo build`) warn about every public
item without a `///` comment; rustdoc also warns about a broken link such as `` [`Sesion`] ``. 0 warnings
means the whole public code is documented. To read the documentation, run `cargo doc --no-deps --open`;
the same pages are online at https://ricard1406.github.io/myTinySql-rust/ (check 12).

## 10. Fresh install

The checks above use your working folder. This one checks that the project also works for someone who
downloads it for the first time:

```bash
cd "$TEST_DIR"
git clone -q https://github.com/ricard1406/myTinySql-rust.git
cd myTinySql-rust
cargo build --release 2>&1 | grep -c '^warning'
cargo test 2>&1 | grep 'test result'
cd "$PROJECT"
```

**You should see** (output, don't type it) `0`, then four summary lines that all say `ok`:

```text
0
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Why this check exists:** a fresh copy has no `target/` folder and nothing left over from earlier work,
and it has only the files that were committed: a file you forgot to `git add` makes this check fail.

## 11. By hand, in a browser

Start the server on the copy of the tables, then open http://localhost:18080 in a browser
(tested with Chromium and Firefox):

```bash
./target/release/myTinySql --http --port=18080 --data="$TEST_DIR/data"
```

| Step | Expected |
| --- | --- |
| Open the page | The tables on the left; EMPLOY (or the last table you used) with its rows. The top bar and the browser tab say **myTinySQL-rust**, "written in Rust" |
| Click **Code documentation** (top right) | A new tab opens https://ricard1406.github.io/myTinySql-rust/: the Rust code documentation ("Crate mytinysql"), not the C++ Doxygen pages |
| Click **COMPANY** on the left | The companies (8, plus the 20 test firms if you ran check 6), each row with **Edit** and **Delete** |
| Click **SYSTEM_TABLE** | Its rows, with no **+ New row** and no Edit/Delete: the catalogue is read-only |
| Click **EMPLOY**; in **Search in** keep NOME, type `fran`, click **Search** | The rows whose NOME contains "fran" (Francesco, Franco) |
| Click **+ New row**, fill every field, with an apostrophe in one (``D'Annunzio``), click **Add row** | A "1 row inserted" message; the form closes and the new row is the last one, apostrophe included. The SQL above the rows shows ``'D'Annunzio'`` as typed |
| On that row, click **Edit**, change a value, press **Escape** | The row is unchanged, and the SQL line above the rows still shows the INSERT: nothing was sent |
| Click **Edit** again, change a value to one with a space (`Reggio Emilia`), press **Enter** | "1 row updated", the new value shown |
| Click **Delete** on it, then **Cancel** in the dialog | Nothing is deleted |
| Click **Delete** again, then the red **Delete** | "1 row deleted", the row is gone |
| In the **SQL console**, type `SELECT * FROM COMPANY WHERE TOWN = Roma` and, on a new line, `SELECT FOO FROM COMPANY`; press Ctrl+Enter | Two results: the rows with a green check, and the error in red |
| Click **+ New table**; name `cars`; first column `model`, Text, 20; click **+ Add column**: `year`, Number, 4 | The SQL under the columns reads `CREATE TABLE CARS (ID NUMBER(10), MODEL CHAR(20), YEAR NUMBER(4))` while you type |
| Click **Create table** | "table CARS created with 3 columns (row length 34)"; CARS appears on the left, open and empty, with **+ New row** |
| Click **+ New table** again, name `cars`, any column, **Create table** | The form stays open and shows "table CARS already exists" in red; click **Cancel** |
| Click **⚙ Settings**, choose **Extra large**, click **Done**, then reload the page | The whole page (text, buttons, spacing) is bigger, and still is after the reload; set it back to **Normal** |
| Switch your computer to a dark theme (optional) | The page follows it, with dark colours |

Above the rows, the page always shows the SQL the last action sent and the server's answer: reading it is
part of the check. Stop the server with Ctrl+C.

The page in `web/` is the C++ version's page with its own name and its own **Code documentation** link
(both in `web/index.html`); everything else is the same, and so is the rest of this checklist.

## 12. On GitHub

GitHub runs two workflows:

- **CI** (`.github/workflows/ci.yml`), after each push to `main` and on every pull request: it builds the
  program, runs `cargo test`, `cargo clippy`, `cargo fmt --check` and `cargo doc`, and fails on any warning.
- **Code documentation** (`.github/workflows/docs.yml`), after each push to `main`: it builds the
  documentation with `cargo doc` and publishes it at https://ricard1406.github.io/myTinySql-rust/, the
  page the **Code documentation** link of the browser page opens.

In the repository's **Actions** tab, the latest run of each must have a green check. From a terminal,
without logging in (the repository is public):

```bash
curl -s "https://api.github.com/repos/ricard1406/myTinySql-rust/actions/workflows/ci.yml/runs?per_page=1" | grep -m2 -E '"(status|conclusion)"'
curl -s "https://api.github.com/repos/ricard1406/myTinySql-rust/actions/workflows/docs.yml/runs?per_page=1" | grep -m2 -E '"(status|conclusion)"'
```

**You should see** (output, don't type it) the same two lines twice:

```text
      "status": "completed",
      "conclusion": "success",
      "status": "completed",
      "conclusion": "success",
```

- `"conclusion": "failure"` means the run failed: open it in the Actions tab to see why.
- `"status": "in_progress"` or `"queued"` means it is still running: try again in a few minutes.
- GitHub answers 60 such requests an hour from one computer; above that the answer says
  `API rate limit exceeded`.

(The C++ version's check 12 looks at the run that publishes its Doxygen pages. This one also checks the
code itself, on a clean machine with the latest Rust.)

## 13. Same as C++

The port's promise is that it behaves exactly like the C++ version. This check runs both programs on every
test script (on the test tables) and every example script (on a copy of `data/`), and compares what they
print and the files they leave. It needs the C++ program: build it in the C++ project with `make`.
If your C++ project is not next to this one, change `CPP` below to its path.

```bash
CPP="$PROJECT/../myTinySQL/myTinySql"
for f in tests/*.sql examples/*.sql; do
    for v in cpp rust; do
        rm -rf "$TEST_DIR/same/$v" && mkdir -p "$TEST_DIR/same/$v"
        case $f in tests/*) cp -r tests/data "$TEST_DIR/same/$v/data" ;; *) cp -r data "$TEST_DIR/same/$v/data" ;; esac
    done
    "$CPP" --data="$TEST_DIR/same/cpp/data" --sqlfile="$f" > "$TEST_DIR/same/cpp/out" 2>&1
    ./target/release/myTinySql --data="$TEST_DIR/same/rust/data" --sqlfile="$f" > "$TEST_DIR/same/rust/out" 2>&1
    diff -q "$TEST_DIR/same/cpp/out" "$TEST_DIR/same/rust/out" > /dev/null && diff -rq "$TEST_DIR/same/cpp/data" "$TEST_DIR/same/rust/data" > /dev/null \
        && echo "$f: same" || echo "$f: DIFFERENT"
done
```

**You should see** (output, don't type it):

```text
tests/apostrophes.sql: same
tests/create.sql: same
tests/delete.sql: same
tests/describe.sql: same
tests/errors.sql: same
tests/insert.sql: same
tests/select.sql: same
tests/update.sql: same
examples/company.sql: same
examples/describe.sql: same
examples/insert.sql: same
examples/select.sql: same
```

The C++ program must be **v0.2.0 with the CREATE TABLE fix** (its `main` from 2026-10-30 on): before that
fix, `tests/create.sql` differs, because of the bug this very comparison found (see the table at the end).
A few differences are on purpose, such as bytes that are not valid UTF-8; [docs/cpp-vs-rust.md](docs/cpp-vs-rust.md)
lists them, and none of them appears in these scripts.

## Clean up

Remove the temporary copy and everything in it:

```bash
rm -rf "$TEST_DIR"
```

## When to run what

| After changing... | Run checks |
| --- | --- |
| Any code | 1, 2, 13 |
| `src/server.rs` or `src/output.rs` | 1, 2, 4, 5, 6, 11, 13 |
| The tables in `data/` | 7 |
| Comments in the code | 9 |
| `Cargo.toml`, the workflow, or before a release | all of them |

## Bugs these checks have caught

| Bug | Found by | Fixed |
| --- | --- | --- |
| In the C++ version, `CREATE TABLE` with huge widths wrapped the row length around to 3, then failed with a wrong message | Comparing both versions on edge cases (now check 13) | C++ [pull request #1](https://github.com/ricard1406/myTinySql/pull/1): the sum stops at the largest number |
| `tests/check_all.sh` reported 2 failed tests for 1, because it counted every line containing `FAILED` | Testing the script with a broken `.expected` file | It counts only the `test ... FAILED` lines |
| INSTALLATION.md showed an error message for a too-old Rust that Cargo does not actually print | Running the command before writing it down | The message is now copied from a real run |
| The browser page still said "myTinySQL", and its **Code documentation** link opened the C++ Doxygen pages | A user checking a fresh install on Ubuntu 26 (now in check 11) | The page says myTinySQL-rust; the Rust documentation is published on GitHub Pages and the link opens it |

As in the C++ version: test scripts and guides are code too, and need testing as well.
