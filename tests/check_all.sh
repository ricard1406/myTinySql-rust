#!/usr/bin/env bash
# Runs every check of TESTING.md, prints one line per check, then a verdict:
#
#   ALL DONE - IT WORKS          every check passed
#   OOOPS - SOMETHING IS WRONG   at least one check failed; the log says what it printed
#
# Usage:  tests/check_all.sh [--offline]
#   --offline  skips check 10 (fresh copy from GitHub) and check 12 (the CI run on GitHub)
#
# Like TESTING.md, it works on a temporary copy of tests/data/: your tables in data/ are only read
# (check 7), never changed. Check 11 needs a person and a browser, so it is only listed.
# Check 13 compares with the C++ version: it needs the C++ program, in ../myTinySQL/myTinySql or
# wherever MYTINYSQL_CPP says; without it, the check is skipped.
# It takes about a minute, mostly for the builds of checks 1, 8 and 10.
set -u

cd "$(dirname "$0")/.."
TEST_DIR=$(mktemp -d)
cp -r tests/data "$TEST_DIR/data"
LOG="$TEST_DIR/check.log"
PORT=18080
URL="http://localhost:$PORT"
BIN=./target/release/myTinySql
CPP_BIN=${MYTINYSQL_CPP:-../myTinySQL/myTinySql}
SERVER=""

offline=false
[[ "${1:-}" == "--offline" ]] && offline=true

# rustup puts cargo in ~/.cargo/bin; use it even if this terminal's PATH does not have it yet.
command -v cargo > /dev/null || { [[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"; }

if [[ -t 1 ]]; then
    green=$'\e[32m' red=$'\e[31m' yellow=$'\e[33m' bold=$'\e[1m' plain=$'\e[0m'
else
    green="" red="" yellow="" bold="" plain=""
fi

passed=0
skipped=0
failed=()
why=""   # set by a check: why it failed or was skipped
note=""  # set by a check: a short note shown after "ok"

# ---------------------------------------------------------------- Helpers

# Stops the test server of checks 4 to 6, if it is running.
stop_server() {
    if [[ -n "$SERVER" ]]; then
        kill "$SERVER" 2> /dev/null
        wait "$SERVER" 2> /dev/null
        SERVER=""
    fi
}
trap stop_server EXIT

# same WHAT EXPECTED GOT: succeeds if GOT is EXPECTED; otherwise logs both and sets the reason.
same() {
    [[ "$3" == "$2" ]] && return 0
    printf 'EXPECTED (%s):\n%s\nGOT:\n%s\n' "$1" "$2" "$3"
    why="$1: expected \"$(tr '\n' ' ' <<< "$2" | sed 's/ $//')\", got \"$(tr '\n' ' ' <<< "$3" | sed 's/ $//')\""
    return 1
}

# needs_server: fails the check if the server of check 4 is not running.
needs_server() {
    [[ -n "$SERVER" ]] && kill -0 "$SERVER" 2> /dev/null && return 0
    why="needs the test server of check 4, which is not running"
    return 1
}

# test_counts: from the output of `cargo test` on standard input, prints the number of unit tests
# that passed (their names have `::`, e.g. `parser::tests::...`), then the number of test scripts.
test_counts() {
    local out
    out=$(cat)
    grep -cE '^test [a-z_]+::[a-z_:]+ \.\.\. ok$' <<< "$out"
    grep -cE '^test [a-z_]+ \.\.\. ok$' <<< "$out"
}

# run NUMBER NAME: runs check_NUMBER, sends what it prints to the log, and prints one result line.
# A check returns 0 (ok), 1 (FAIL) or 2 (skipped).
run() {
    local number=$1 name=$2 status
    printf '%3s  %-24s ' "$number" "$name"
    printf '\n===== %s. %s\n' "$number" "$name" >> "$LOG"
    why="" note=""
    "check_$number" >> "$LOG" 2>&1
    status=$?
    case $status in
        0) echo "${green}ok${plain}${note:+   $note}"; passed=$((passed + 1)) ;;
        2) echo "${yellow}skipped${plain}   $why"; skipped=$((skipped + 1)) ;;
        *) echo "${red}FAIL${plain}   ${why:-see the log}"; failed+=("$number") ;;
    esac
    echo "result: $status $why" >> "$LOG"
}

# ---------------------------------------------------------------- The checks

check_1() {  # Clean build: no compiler warnings, no linter warnings, formatted
    cargo clean
    local out lint format
    out=$(cargo build --release 2>&1)
    echo "$out"
    [[ -x "$BIN" ]] || { why="the build failed"; return 1; }
    same "compiler warnings" 0 "$(grep -c '^warning' <<< "$out")" || return 1
    lint=$(cargo clippy --all-targets 2>&1)
    echo "$lint"
    same "linter (clippy) warnings" 0 "$(grep -c '^warning' <<< "$lint")" || return 1
    format=$(cargo fmt --check 2>&1)
    echo "$format"
    same "files cargo fmt would change" 0 "$(grep -c '^Diff in' <<< "$format")"
}

check_2() {  # Automated tests: every unit test and every test script passes
    local out scripts counts
    out=$(cargo test 2>&1)
    echo "$out"
    scripts=$(ls tests/*.sql | wc -l)
    counts=$(test_counts <<< "$out")
    note="$(head -1 <<< "$counts") unit tests, $scripts test scripts"
    same "failed tests" 0 "$(grep -cE '^test .* \.\.\. FAILED$' <<< "$out")" || return 1
    same "test scripts passed" "$scripts" "$(tail -1 <<< "$counts")"
}

check_3() {  # Command line: rows, an error, and exit code 1
    local rows out code
    rows=$(echo "SELECT ID,NOME,COGNOME FROM EMPLOY WHERE NOME LIKE '%fran%'" \
        | "$BIN" --data="$TEST_DIR/data" --sqlfile=- | sed -n '3,5p' | tr -s ' ' | sed 's/ $//')
    echo "$rows"
    same "first rows of the LIKE search" $'11 Francesco Cavalli\n16 Francesco Gasparini\n17 Francesco Geminiani' "$rows" \
        || return 1
    out=$(echo "SELECT FOO FROM EMPLOY" | "$BIN" --data="$TEST_DIR/data" --sqlfile=-)
    code=$?
    echo "$out"
    same "error message" "ERROR: unknown column 'FOO' in table EMPLOY" "$(sed -n 2p <<< "$out")" || return 1
    same "exit code" 1 "$code"
}

check_4() {  # Browser server: the answers of the six requests
    "$BIN" --http --port=$PORT --data="$TEST_DIR/data" > "$TEST_DIR/server.log" 2>&1 &
    SERVER=$!
    sleep 1
    if ! kill -0 "$SERVER" 2> /dev/null; then
        SERVER=""
        why="the server did not start: $(head -1 "$TEST_DIR/server.log")"
        return 1
    fi
    local got
    got=$(
        curl -s -o /dev/null -w '%{http_code}\n' "$URL/"
        curl -s -o /dev/null -w '%{http_code}\n' "$URL/favicon.ico"
        curl -s -o /dev/null -w '%{http_code}\n' -X PUT "$URL/api/sql"
        curl -s -o /dev/null -w '%{http_code} %{content_type}\n' "$URL/api/tables"
        curl -s --data-binary 'DESCRIBE' "$URL/api/sql" | grep -o '"message":"[^"]*"'
        curl -s -o /dev/null -w '%{http_code}\n' --path-as-is "$URL/../data/SYSTEM_TABLE"
    )
    same "the six answers" $'200\n404\n405\n200 application/json\n"message":"4 tables"\n404' "$got"
}

check_5() {  # Idle connection: other requests are not blocked, and it is served after 6 s
    needs_server || return 1
    local time first
    exec 3<> "/dev/tcp/localhost/$PORT" || { why="cannot open a connection to port $PORT"; return 1; }
    time=$(curl -s -o /dev/null -w '%{time_total}' "$URL/")
    echo "other request while idle: ${time}s"
    sleep 6
    printf 'GET /api/tables HTTP/1.1\r\nHost: x\r\n\r\n' >&3
    first=$(timeout 10 head -1 <&3 | tr -d '\r')
    exec 3<&-
    echo "$first"
    LC_ALL=C awk -v t="$time" 'BEGIN { exit !(t < 1) }' || { why="another request waited ${time}s"; return 1; }
    note="other request answered in ${time}s"
    same "answer on the idle connection" "HTTP/1.1 200 OK" "$first"
}

check_6() {  # Parallel writes: 20 INSERTs at once give 28 rows, no duplicate ID, one row length
    needs_server || return 1
    (
        for i in $(seq 1 20); do
            curl -s -o /dev/null --data-binary "INSERT INTO COMPANY VALUES ('Firm $i',Test,'Via $i',Roma)" "$URL/api/sql" &
        done
        wait
    )
    local file="$TEST_DIR/data/COMPANY_DATA" got
    got=$(
        wc -l < "$file"
        cut -c1-10 "$file" | sort | uniq -d | wc -l
        LC_ALL=C awk '{print length($0)}' "$file" | sort -u
    )
    stop_server
    same "rows, duplicate IDs, row lengths" $'28\n0\n250' "$got"
}

check_7() {  # File integrity: every file in data/ has rows of exactly its length in SYSTEM_TABLE
    local f name want lengths bad=0
    for f in data/*; do
        name=$(basename "$f")
        if [[ "$name" == SYSTEM_TABLE ]]; then
            want=80
        else
            # RECLEN (characters 75-79) of the table's lines; TABLE is characters 15-34
            want=$(LC_ALL=C awk -v t="${name%_DATA}" \
                '{ n = substr($0, 16, 20); sub(/ +$/, "", n); if (n == t) { print substr($0, 76, 5) + 0; exit } }' data/SYSTEM_TABLE)
        fi
        lengths=$(LC_ALL=C awk '{print length($0)}' "$f" | sort -u | tr '\n' ' ' | sed 's/ $//')
        echo "$name: ${lengths:-(no rows)} (expected ${want:-?})"
        if [[ -z "$want" ]]; then
            bad=1 why="$name: no table ${name%_DATA} in SYSTEM_TABLE"
        elif [[ -n "$lengths" && "$lengths" != "$want" ]]; then
            bad=1 why="$name has rows of length $lengths instead of $want"
        fi
    done
    note="$(ls data | wc -l) files"
    return $bad
}

check_8() {  # Memory safety: no unsafe code, and every test script on the checked (debug) build
    local f forbids unsafe problems total=0
    forbids=$(grep -l '^#!\[forbid(unsafe_code)\]' src/lib.rs src/main.rs | wc -l)
    same "files with #![forbid(unsafe_code)]" 2 "$forbids" || return 1
    unsafe=$(grep -rnE '\bunsafe *(\{|fn|impl|trait)' src | grep -v '^[^:]*:[0-9]*: *//')
    echo "unsafe code: ${unsafe:-none}"
    same "unsafe blocks" "" "$unsafe" || return 1
    cargo build || { why="the debug build failed"; return 1; }
    for f in tests/*.sql; do
        rm -rf "$TEST_DIR/debug" && cp -r tests/data "$TEST_DIR/debug"
        target/debug/myTinySql --data="$TEST_DIR/debug" --sqlfile="$f" > /dev/null 2> "$TEST_DIR/debug.log"
        problems=$(grep -c 'panicked' "$TEST_DIR/debug.log")
        echo "$f: $problems problems"
        [[ "$problems" == 0 ]] || cat "$TEST_DIR/debug.log"
        total=$((total + problems))
    done
    same "panics on the debug build" 0 "$total"
}

check_9() {  # Documentation: cargo doc gives no warnings
    local out
    out=$(cargo doc --no-deps 2>&1)
    echo "$out"
    same "documentation warnings" 0 "$(grep -c '^warning' <<< "$out")"
}

check_10() {  # Fresh install: a new copy from GitHub builds without warnings and passes its tests
    $offline && { why="--offline"; return 2; }
    git clone -q https://github.com/ricard1406/myTinySql-rust.git "$TEST_DIR/myTinySql-rust" \
        || { why="git clone failed (no internet connection?)"; return 1; }
    local build tests scripts
    build=$(cd "$TEST_DIR/myTinySql-rust" && cargo build --release 2>&1)
    echo "$build"
    same "compiler warnings" 0 "$(grep -c '^warning' <<< "$build")" || return 1
    tests=$(cd "$TEST_DIR/myTinySql-rust" && cargo test 2>&1)
    echo "$tests"
    scripts=$(ls "$TEST_DIR"/myTinySql-rust/tests/*.sql | wc -l)
    note="the version on GitHub, $scripts test scripts"
    same "failed tests" 0 "$(grep -cE '^test .* \.\.\. FAILED$' <<< "$tests")" || return 1
    same "test scripts passed" "$scripts" "$(test_counts <<< "$tests" | tail -1)"
}

check_12() {  # On GitHub: the latest CI run succeeded
    $offline && { why="--offline"; return 2; }
    # The repository is public, so GitHub's API answers without logging in (60 requests an hour).
    local api="https://api.github.com/repos/ricard1406/myTinySql-rust/actions/workflows/ci.yml/runs?per_page=1"
    local answer status conclusion commit
    answer=$(curl -s --max-time 20 "$api") || { why="could not reach api.github.com"; return 1; }
    if grep -q 'rate limit' <<< "$answer"; then
        why="GitHub answers 60 requests an hour without logging in: try again later"
        return 2
    fi
    status=$(grep -m1 '"status"' <<< "$answer" | sed -E 's/.*: *"?([^",]*)"?,?$/\1/')
    conclusion=$(grep -m1 '"conclusion"' <<< "$answer" | sed -E 's/.*: *"?([^",]*)"?,?$/\1/')
    commit=$(grep -m1 '"head_sha"' <<< "$answer" | sed -E 's/.*: *"([0-9a-f]{7}).*/\1/')
    echo "latest run: status $status, conclusion $conclusion, commit $commit"
    [[ -n "$status" ]] || { echo "$answer"; why="unexpected answer from api.github.com"; return 1; }
    [[ "$status" == completed ]] || { why="the latest run is still running: try again in a few minutes"; return 2; }
    note="CI of commit $commit"
    same "latest CI run" "completed success" "$status $conclusion"
}

check_13() {  # Same as C++: both programs print the same and write the same files
    [[ -x "$CPP_BIN" ]] || { why="no C++ program at $CPP_BIN (set MYTINYSQL_CPP to its path)"; return 2; }
    local f v name differ=0 compared=0
    # The test scripts run on the frozen test tables; the examples on a copy of data/.
    for f in tests/*.sql examples/*.sql; do
        name=$(basename "$f" .sql)
        for v in cpp rust; do
            rm -rf "$TEST_DIR/same/$v" && mkdir -p "$TEST_DIR/same/$v"
            if [[ "$f" == tests/* ]]; then cp -r tests/data "$TEST_DIR/same/$v/data"; else cp -r data "$TEST_DIR/same/$v/data"; fi
        done
        "$CPP_BIN" --data="$TEST_DIR/same/cpp/data" --sqlfile="$f" > "$TEST_DIR/same/cpp/out" 2>&1
        "$BIN" --data="$TEST_DIR/same/rust/data" --sqlfile="$f" > "$TEST_DIR/same/rust/out" 2>&1
        if diff "$TEST_DIR/same/cpp/out" "$TEST_DIR/same/rust/out" && diff -r "$TEST_DIR/same/cpp/data" "$TEST_DIR/same/rust/data"; then
            echo "$f: same output, same files"
        else
            echo "$f: DIFFERENT"
            differ=$((differ + 1))
        fi
        compared=$((compared + 1))
    done
    note="$compared scripts, against $CPP_BIN"
    same "scripts that differ" 0 "$differ"
}

# ---------------------------------------------------------------- Main

echo "${bold}myTinySQL in Rust: running the checks of TESTING.md${plain} (about a minute)"
echo

# The tools the checks need, as in "What you need" of TESTING.md
needed=(cargo cc curl)
$offline || needed+=(git)
missing=()
for tool in "${needed[@]}"; do
    command -v "$tool" > /dev/null || missing+=("$tool")
done
if (( ${#missing[@]} > 0 )); then
    echo "${red}${bold}OOOPS - SOMETHING IS WRONG${plain}"
    echo "Missing tools: ${missing[*]}."
    if [[ " ${missing[*]} " == *" cargo "* ]]; then
        echo "Install Rust as INSTALLATION.md shows (step 1), then run this script again."
    fi
    packages=$(printf '%s\n' "${missing[@]}" | grep -v '^cargo$' | sed 's/^cc$/build-essential/' | tr '\n' ' ')
    if [[ -n "${packages// }" ]]; then
        echo "Install the others with:"
        echo
        echo "    sudo apt install $packages"
    fi
    rm -rf "$TEST_DIR"
    exit 1
fi

run 1 "Clean build"
if [[ ! -x "$BIN" ]]; then
    echo
    echo "${red}${bold}OOOPS - SOMETHING IS WRONG${plain}   the program does not build, so nothing else can be checked"
    echo "What the build printed: $LOG"
    exit 1
fi
run 2 "Automated tests"
run 3 "Command line"
run 4 "Browser server"
run 5 "Idle connection"
run 6 "Parallel writes"
run 7 "File integrity"
run 8 "Memory safety"
run 9 "Documentation"
run 10 "Fresh install"
printf '%3s  %-24s %s\n' 11 "By hand, in a browser" "--   by a person: see TESTING.md, check 11"
run 12 "On GitHub"
run 13 "Same as C++"

echo
if (( ${#failed[@]} == 0 )); then
    echo "${green}${bold}ALL DONE - IT WORKS${plain}   ($passed checks passed$( (( skipped > 0 )) && echo ", $skipped skipped"))"
    rm -rf "$TEST_DIR"
    exit 0
fi
echo "${red}${bold}OOOPS - SOMETHING IS WRONG${plain}   (failed: check ${failed[*]})"
echo "What every check printed: $LOG"
echo "The temporary copy stays in $TEST_DIR; remove it with: rm -rf \"$TEST_DIR\""
exit 1
