//! The tokenizer and recursive-descent parser for the SQL dialect.
//!
//! The grammar:
//!
//! ```text
//! SELECT <item>[,<item>...] FROM <table> [WHERE <col> <op> <value>]
//! INSERT INTO <table> VALUES (<value>[,<value>...])
//! UPDATE <table> SET <col> = <value>[,<col> = <value>...] WHERE <col> <op> <value>
//! DELETE FROM <table> WHERE <col> <op> <value>
//! CREATE TABLE <table> (<col> CHAR(<width>)|NUMBER(<width>)[,...])
//! COLUMN <col> FORMAT A<width>
//! DESCRIBE [<table>]
//! REM <comment>    -- <comment>
//! ```
//!
//! `<op>` is one of `=` `!=` `<>` `<` `<=` `>` `>=` or `LIKE` (with `%` and `_` wildcards).
//! Keywords are case-insensitive. A trailing `;` or `@` is ignored.
//!
//! A `'` between two letters or digits is an apostrophe, part of the word, as in d'Asti or L'Aquila:
//! it never starts or ends a quoted string. `''` inside quotes still stands for one quote.
//!
//! The tokenizer looks at bytes (`&[u8]`), like the C++ code looks at `char`s. It only ever cuts the
//! text next to an ASCII character such as a space, a comma or a quote, so every piece it cuts is
//! still valid UTF-8 (a Rust `&str` must always be).

use crate::statement::{
    Assignment, ColumnDefinition, ColumnFormatStmt, Condition, CreateTableStmt, DeleteStmt, DescribeStmt, InsertStmt,
    Operator, SelectItem, SelectStmt, Statement, UpdateStmt,
};
use crate::util::{SqlError, SqlResult, parse_integer, trim};

/// One token of a statement. (In C++, a `TokenKind` and a `text`; here an enum that carries the text.)
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// A keyword, name or bare value, e.g. `SELECT`, `EMPLOY`, `Torino`, `42`.
    Word(String),
    /// A `'quoted string'`, without its quotes.
    Quoted(String),
    /// Punctuation or an operator: `( ) , * ; @ = != <> < <= > >=`.
    Symbol(String),
    /// The end of the statement.
    End,
}

impl Token {
    /// The token's text: empty for [`Token::End`].
    fn text(&self) -> &str {
        match self {
            Token::Word(text) | Token::Quoted(text) | Token::Symbol(text) => text,
            Token::End => "",
        }
    }

    /// Describes the token for an error message: e.g. `"FROM"`, `'text'`, or "end of statement".
    fn describe(&self) -> String {
        match self {
            Token::End => "end of statement".to_string(),
            Token::Quoted(text) => format!("'{text}'"),
            Token::Word(text) | Token::Symbol(text) => format!("\"{text}\""),
        }
    }
}

/// The characters that end a word (a `'` only when it is not an apostrophe, see [`is_apostrophe`]).
const DELIMITERS: &[u8] = b" \t\r\n(),*;@=<>!'";

/// Tells whether a byte can stand next to an apostrophe inside a word:
/// an ASCII letter or digit, or any byte of a UTF-8 character such as `è`.
fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c >= 0x80
}

/// Tells whether the `'` at position `i` is an apostrophe inside a word, as in d'Asti, L'Aquila or dell'800.
///
/// Italian (and other languages) put apostrophes inside names, so a `'` with a letter or digit on both
/// sides is part of the word, in quotes or not. Only a `'` next to a space, a comma, a bracket or the
/// end of the text starts or ends a quoted string.
fn is_apostrophe(text: &[u8], i: usize) -> bool {
    i > 0 && i + 1 < text.len() && text[i] == b'\'' && is_word_char(text[i - 1]) && is_word_char(text[i + 1])
}

/// Splits a statement into tokens, always ending with one [`Token::End`].
///
/// Fails on an unterminated quoted string or a lone `!`.
fn tokenize(sql: &str) -> SqlResult<Vec<Token>> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'\'' => {
                // 'quoted string', with '' standing for one quote; an apostrophe (d'Asti) does not end it
                let mut text = Vec::new();
                let after_word = i > 0 && is_word_char(bytes[i - 1]); // e.g. Foscari' in 'Ca' Foscari'
                i += 1;
                loop {
                    let Some(&b) = bytes.get(i) else {
                        return Err(SqlError::new(if after_word {
                            "unterminated quoted string (an apostrophe at the end of a word, \
                             as in Ca' Foscari, is written twice inside quotes: 'Ca'' Foscari')"
                        } else {
                            "unterminated quoted string"
                        }));
                    };
                    if b == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            text.push(b'\'');
                            i += 2;
                            continue;
                        }
                        if !is_apostrophe(bytes, i) {
                            i += 1;
                            break;
                        }
                    }
                    text.push(b);
                    i += 1;
                }
                // Only ASCII quotes were left out, so the bytes are still valid UTF-8.
                tokens.push(Token::Quoted(String::from_utf8(text).expect("quotes are ASCII")));
            }
            b'(' | b')' | b',' | b'*' | b';' | b'@' => {
                tokens.push(Token::Symbol(char::from(c).to_string()));
                i += 1;
            }
            b'=' | b'<' | b'>' | b'!' => match bytes.get(i..i + 2) {
                Some(b"<=" | b">=" | b"!=" | b"<>") => {
                    tokens.push(Token::Symbol(sql[i..i + 2].to_string()));
                    i += 2;
                }
                _ if c == b'!' => return Err(SqlError::new("unexpected '!'")),
                _ => {
                    tokens.push(Token::Symbol(char::from(c).to_string()));
                    i += 1;
                }
            },
            _ => {
                // A word runs to the next delimiter; an apostrophe inside it (L'Aquila) is part of the word.
                let mut stop = i;
                while stop < bytes.len() && (!DELIMITERS.contains(&bytes[stop]) || is_apostrophe(bytes, stop)) {
                    stop += 1;
                }
                tokens.push(Token::Word(sql[i..stop].to_string()));
                i = stop;
            }
        }
    }
    tokens.push(Token::End);
    Ok(tokens)
}

/// The comparison operator a symbol stands for, if any: `=` `!=` `<>` `<` `<=` `>` `>=`.
fn comparison(symbol: &str) -> Option<Operator> {
    match symbol {
        "=" => Some(Operator::Equal),
        "!=" | "<>" => Some(Operator::NotEqual),
        "<" => Some(Operator::Less),
        "<=" => Some(Operator::LessOrEqual),
        ">" => Some(Operator::Greater),
        ">=" => Some(Operator::GreaterOrEqual),
        _ => None,
    }
}

/// A recursive-descent parser: one method per statement type, reading the tokens left to right.
struct Parser {
    /// The tokens, ending with [`Token::End`].
    tokens: Vec<Token>,
    /// The position of the next token to read.
    pos: usize,
}

impl Parser {
    /// Prepares to parse a statement's tokens, from [`tokenize`].
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    /// Parses the whole statement, choosing the method from its first word.
    fn parse(&mut self) -> SqlResult<Statement> {
        let Token::Word(word) = self.peek() else {
            return self.fail("a command");
        };
        match word.to_ascii_uppercase().as_str() {
            "SELECT" => self.parse_select().map(Statement::Select),
            "INSERT" => self.parse_insert().map(Statement::Insert),
            "UPDATE" => self.parse_update().map(Statement::Update),
            "DELETE" => self.parse_delete().map(Statement::Delete),
            "CREATE" => self.parse_create_table().map(Statement::CreateTable),
            "COLUMN" => self.parse_column().map(Statement::ColumnFormat),
            "DESCRIBE" => self.parse_describe().map(Statement::Describe),
            _ => Err(SqlError::new(format!("unknown command \"{word}\""))),
        }
    }

    /// Looks at the next token without consuming it.
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    /// The next token's text if it is a word, without consuming it.
    fn peek_word(&self) -> Option<&str> {
        match self.peek() {
            Token::Word(text) => Some(text),
            _ => None,
        }
    }

    /// Consumes the next token and returns its text (the End token is never consumed).
    fn next_text(&mut self) -> String {
        let text = self.peek().text().to_string();
        if *self.peek() != Token::End {
            self.pos += 1;
        }
        text
    }

    /// Reports a syntax error at the current token: "expected ... but found ...".
    ///
    /// C++ throws here (`[[noreturn]]`); in Rust the caller writes `return self.fail(...)`.
    /// The `<T>` lets it stand for any result type, whatever the caller returns.
    fn fail<T>(&self, expected: &str) -> SqlResult<T> {
        Err(SqlError::new(format!("expected {expected} but found {}", self.peek().describe())))
    }

    /// Consumes the next token if it is a given keyword (case-insensitive); tells whether it did.
    fn accept_keyword(&mut self, keyword: &str) -> bool {
        let found = self.peek_word().is_some_and(|word| word.eq_ignore_ascii_case(keyword));
        if found {
            self.pos += 1;
        }
        found
    }

    /// Consumes a keyword that must come next.
    fn expect_keyword(&mut self, keyword: &str) -> SqlResult<()> {
        if self.accept_keyword(keyword) { Ok(()) } else { self.fail(keyword) }
    }

    /// Consumes the next token if it is a given symbol, e.g. `,`; tells whether it did.
    fn accept_symbol(&mut self, symbol: &str) -> bool {
        let found = matches!(self.peek(), Token::Symbol(text) if text == symbol);
        if found {
            self.pos += 1;
        }
        found
    }

    /// Consumes a symbol that must come next, e.g. `(`.
    fn expect_symbol(&mut self, symbol: &str) -> SqlResult<()> {
        if self.accept_symbol(symbol) { Ok(()) } else { self.fail(&format!("'{symbol}'")) }
    }

    /// Consumes a word (a name) that must come next; `what` is for the error message, e.g. "a column name".
    fn expect_word(&mut self, what: &str) -> SqlResult<String> {
        match self.peek() {
            Token::Word(_) => Ok(self.next_text()),
            _ => self.fail(what),
        }
    }

    /// Consumes a value that must come next: a word or a quoted string.
    fn expect_value(&mut self) -> SqlResult<String> {
        match self.peek() {
            Token::Word(_) | Token::Quoted(_) => Ok(self.next_text()),
            _ => self.fail("a value"),
        }
    }

    /// Checks that the statement ends here, after any trailing `;` or `@`.
    fn expect_end(&mut self) -> SqlResult<()> {
        while self.accept_symbol(";") || self.accept_symbol("@") {}
        if *self.peek() == Token::End { Ok(()) } else { self.fail("end of statement") }
    }

    /// Parses `<column> <op> <value>`, after the WHERE keyword; `<op>` is a comparison or `LIKE`.
    fn parse_condition(&mut self) -> SqlResult<Condition> {
        let column = self.expect_word("a column name")?;
        let op = if self.accept_keyword("LIKE") {
            Operator::Like
        } else {
            let op = match self.peek() {
                Token::Symbol(symbol) => comparison(symbol),
                _ => None,
            };
            let Some(op) = op else {
                return self.fail("a comparison (= != < <= > >= or LIKE)");
            };
            self.pos += 1;
            op
        };
        let value = self.expect_value()?;
        if let Some(word) = self.peek_word() {
            if word.eq_ignore_ascii_case("AND") || word.eq_ignore_ascii_case("OR") {
                return Err(SqlError::new(format!(
                    "WHERE takes one condition: {} is not supported",
                    word.to_ascii_uppercase()
                )));
            }
            let mut words = value;
            while let Some(word) = self.peek_word() {
                words.push(' ');
                words.push_str(word);
                self.pos += 1;
            }
            return Err(SqlError::new(format!("put a value with spaces in quotes, e.g. {column} = '{words}'")));
        }
        Ok(Condition { column, op, value })
    }

    /// Parses `SELECT <items> FROM <table> [WHERE ...]`.
    fn parse_select(&mut self) -> SqlResult<SelectStmt> {
        self.expect_keyword("SELECT")?;
        let mut items = Vec::new();
        loop {
            if self.accept_symbol("*") {
                items.push(SelectItem::Star);
            } else if let Token::Quoted(_) = self.peek() {
                items.push(SelectItem::Literal(self.next_text()));
            } else {
                items.push(SelectItem::Column(self.expect_word("a column name")?));
            }
            if !self.accept_symbol(",") {
                break;
            }
        }

        self.expect_keyword("FROM")?;
        let table = self.expect_word("a table name")?;

        let condition = if self.accept_keyword("WHERE") { Some(self.parse_condition()?) } else { None };

        self.expect_end()?;
        Ok(SelectStmt { items, table, condition })
    }

    /// Parses `INSERT INTO <table> VALUES (...)`.
    fn parse_insert(&mut self) -> SqlResult<InsertStmt> {
        self.expect_keyword("INSERT")?;
        self.expect_keyword("INTO")?;
        let table = self.expect_word("a table name")?;
        self.expect_keyword("VALUES")?;
        self.expect_symbol("(")?;
        let mut values = Vec::new();
        loop {
            // A value is a 'quoted string', or one or more bare words joined by single spaces.
            if let Token::Quoted(_) = self.peek() {
                values.push(self.next_text());
            } else {
                let mut value = String::new();
                while let Some(word) = self.peek_word() {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(word);
                    self.pos += 1;
                }
                values.push(value);
            }
            if !self.accept_symbol(",") {
                break;
            }
        }
        self.expect_symbol(")")?;
        self.expect_end()?;
        Ok(InsertStmt { table, values })
    }

    /// Parses `UPDATE <table> SET <col> = <value>, ... WHERE ...`.
    fn parse_update(&mut self) -> SqlResult<UpdateStmt> {
        self.expect_keyword("UPDATE")?;
        let table = self.expect_word("a table name")?;
        self.expect_keyword("SET")?;
        let mut assignments = Vec::new();
        loop {
            let column = self.expect_word("a column name")?;
            self.expect_symbol("=")?;
            let value = self.expect_value()?;
            if let Some(word) = self.peek_word().filter(|word| !word.eq_ignore_ascii_case("WHERE")) {
                return Err(SqlError::new(format!(
                    "put a value with spaces in quotes, e.g. {column} = '{value} {word}'"
                )));
            }
            assignments.push(Assignment { column, value });
            if !self.accept_symbol(",") {
                break;
            }
        }
        if !self.accept_keyword("WHERE") {
            return self.fail("WHERE (UPDATE always needs a condition, e.g. WHERE ID = 3)");
        }
        let condition = self.parse_condition()?;
        self.expect_end()?;
        Ok(UpdateStmt { table, assignments, condition })
    }

    /// Parses `DELETE FROM <table> WHERE ...`.
    fn parse_delete(&mut self) -> SqlResult<DeleteStmt> {
        self.expect_keyword("DELETE")?;
        self.expect_keyword("FROM")?;
        let table = self.expect_word("a table name")?;
        if !self.accept_keyword("WHERE") {
            return self.fail("WHERE (DELETE always needs a condition, e.g. WHERE ID = 3)");
        }
        let condition = self.parse_condition()?;
        self.expect_end()?;
        Ok(DeleteStmt { table, condition })
    }

    /// Parses `CREATE TABLE <table> (<col> <type>(<width>), ...)`, where `<type>` is CHAR or NUMBER.
    fn parse_create_table(&mut self) -> SqlResult<CreateTableStmt> {
        self.expect_keyword("CREATE")?;
        self.expect_keyword("TABLE")?;
        let table = self.expect_word("a table name")?;
        self.expect_symbol("(")?;
        let mut columns = Vec::new();
        loop {
            let name = self.expect_word("a column name")?;
            let kind = match self.expect_word("a type (CHAR or NUMBER)")?.to_ascii_uppercase().as_str() {
                "CHAR" => "S1",
                "NUMBER" => "N1",
                other => {
                    return Err(SqlError::new(format!(
                        "unknown type \"{other}\" for column {name}: use CHAR(<width>) or NUMBER(<width>)"
                    )));
                }
            };
            self.expect_symbol("(")?;
            let width = self.expect_word("a width")?;
            // `usize::try_from` refuses a negative number, as `*value < 1` does in C++ for 0 and below.
            let Some(width) = parse_integer(&width).filter(|n| *n >= 1).and_then(|n| usize::try_from(n).ok()) else {
                return Err(SqlError::new(format!(
                    "invalid width \"{width}\" for column {name}: expected a positive number"
                )));
            };
            self.expect_symbol(")")?;
            columns.push(ColumnDefinition { name, kind: kind.to_string(), width });
            if !self.accept_symbol(",") {
                break;
            }
        }
        self.expect_symbol(")")?;
        self.expect_end()?;
        Ok(CreateTableStmt { table, columns })
    }

    /// Parses `COLUMN <col> FORMAT A<width>`.
    fn parse_column(&mut self) -> SqlResult<ColumnFormatStmt> {
        self.expect_keyword("COLUMN")?;
        let column = self.expect_word("a column name")?;
        self.expect_keyword("FORMAT")?;
        let format = self.expect_word("a format such as A16")?;
        // `strip_prefix` checks the `A` first, so the cut after it is always at a character boundary.
        let width = format
            .strip_prefix(['A', 'a'])
            .and_then(parse_integer)
            .filter(|n| *n >= 1)
            .and_then(|n| usize::try_from(n).ok());
        let Some(width) = width else {
            return Err(SqlError::new(format!("invalid format \"{format}\", expected A<width> such as A16")));
        };
        self.expect_end()?;
        Ok(ColumnFormatStmt { column, width })
    }

    /// Parses `DESCRIBE [<table>]`.
    fn parse_describe(&mut self) -> SqlResult<DescribeStmt> {
        self.expect_keyword("DESCRIBE")?;
        let table = if self.peek_word().is_some() { Some(self.next_text()) } else { None };
        self.expect_end()?;
        Ok(DescribeStmt { table })
    }
}

/// Tells whether a line is a comment or empty: an empty line, `-- ...`, or `REM ...`.
fn is_comment(sql: &str) -> bool {
    let sql = trim(sql);
    if sql.is_empty() || sql.starts_with("--") {
        return true;
    }
    let first = sql.split([' ', '\t']).next().unwrap_or_default();
    first.eq_ignore_ascii_case("REM")
}

/// Parses one statement; a trailing `;` is allowed.
///
/// Fails with an [`SqlError`] on a syntax error; the message says what was expected and what was found.
pub fn parse_statement(sql: &str) -> SqlResult<Statement> {
    if is_comment(sql) {
        return Ok(Statement::Comment);
    }
    Parser::new(tokenize(sql)?).parse()
}

/// Splits a script into statements: one per line, and `;` outside quotes also ends one.
///
/// Returns the statements in order, trimmed; comment lines and empty pieces are left out.
pub fn split_statements(script: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut add = |piece: &str| {
        let piece = trim(piece);
        if !piece.is_empty() {
            statements.push(piece.to_string());
        }
    };
    for line in script.split('\n') {
        if is_comment(line) {
            continue;
        }
        let bytes = line.as_bytes();
        let mut in_quotes = false;
        let mut start = 0;
        for (i, &c) in bytes.iter().enumerate() {
            if c == b'\'' && !is_apostrophe(bytes, i) {
                in_quotes = !in_quotes;
            }
            if c == b';' && !in_quotes {
                add(&line[start..i]);
                start = i + 1;
            }
        }
        add(&line[start..]);
    }
    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apostrophes_stay_inside_words() {
        let tokens = tokenize("CITTA = L'Aquila").unwrap();
        assert_eq!(tokens[2], Token::Word("L'Aquila".to_string()));
        let tokens = tokenize("'Barbera d'Asti' 'it''s'").unwrap();
        assert_eq!(tokens[0], Token::Quoted("Barbera d'Asti".to_string()));
        assert_eq!(tokens[1], Token::Quoted("it's".to_string()));
    }

    #[test]
    fn parses_a_select() {
        let Statement::Select(stmt) = parse_statement("select ID, 'x' from EMPLOY where ID <= 3;").unwrap() else {
            panic!("not a SELECT");
        };
        assert_eq!(stmt.items, vec![SelectItem::Column("ID".into()), SelectItem::Literal("x".into())]);
        assert_eq!(stmt.condition.unwrap().op, Operator::LessOrEqual);
    }

    #[test]
    fn splits_at_semicolons_outside_quotes() {
        let script = "REM skip; me\nSELECT 'a;b' FROM T; DESCRIBE\n\n-- and me";
        assert_eq!(split_statements(script), vec!["SELECT 'a;b' FROM T", "DESCRIBE"]);
    }

    #[test]
    fn reports_what_it_expected() {
        let error = parse_statement("SELECT NOME FROM EMPLOY WHERE").unwrap_err();
        assert_eq!(error.to_string(), "expected a column name but found end of statement");
    }
}
