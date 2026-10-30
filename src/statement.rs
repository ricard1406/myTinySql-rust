//! The parsed form of every statement.
//!
//! In C++, `Statement` is a `std::variant` of structs. In Rust it is an `enum`: each variant can
//! carry its own data, and a `match` on it must handle every variant, or the program does not compile.

/// One entry of a SELECT list.
///
/// C++ needs a struct with a `Kind` and a `text` that means different things for each kind;
/// a Rust enum carries the text only where there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    /// `*`: every column of the table.
    Star,
    /// A column name.
    Column(String),
    /// A `'quoted text'`, printed as-is on every row.
    Literal(String),
}

/// The operator of a WHERE test. (C++ keeps it as a string: `"="`, `"<>"`, `"LIKE"`...)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// `=`
    Equal,
    /// `!=` or `<>`
    NotEqual,
    /// `<`
    Less,
    /// `<=`
    LessOrEqual,
    /// `>`
    Greater,
    /// `>=`
    GreaterOrEqual,
    /// `LIKE`, with the `%` and `_` wildcards.
    Like,
}

/// A `WHERE <column> <op> <value>` test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    /// The column to test.
    pub column: String,
    /// How to compare.
    pub op: Operator,
    /// The value to compare with (a number when both sides are integers), or the `LIKE` pattern.
    pub value: String,
}

/// `SELECT <items> FROM <table> [WHERE <condition>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectStmt {
    /// What to show, in order.
    pub items: Vec<SelectItem>,
    /// The table to read.
    pub table: String,
    /// The optional WHERE test. (`where` is a keyword in Rust, so the field has another name.)
    pub condition: Option<Condition>,
}

/// `INSERT INTO <table> VALUES (<values>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertStmt {
    /// The table to add a row to.
    pub table: String,
    /// The values, without the ID when it is assigned automatically.
    pub values: Vec<String>,
}

/// `DELETE FROM <table> WHERE <condition>`. The WHERE is required, so a table is never emptied by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteStmt {
    /// The table to remove rows from.
    pub table: String,
    /// Which rows to remove.
    pub condition: Condition,
}

/// One `<column> = <value>` of an UPDATE's SET list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// The column to change.
    pub column: String,
    /// Its new value.
    pub value: String,
}

/// `UPDATE <table> SET <col> = <value>[, ...] WHERE <condition>`. The WHERE is required, as for DELETE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStmt {
    /// The table to change.
    pub table: String,
    /// The columns to set and their new values.
    pub assignments: Vec<Assignment>,
    /// Which rows to change.
    pub condition: Condition,
}

/// `COLUMN <name> FORMAT A<width>`: the console display width of a column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnFormatStmt {
    /// The column name.
    pub column: String,
    /// Display width in characters.
    pub width: usize,
}

/// One column of a CREATE TABLE: `<name> CHAR(<width>)` or `<name> NUMBER(<width>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDefinition {
    /// The column name.
    pub name: String,
    /// `S1` for CHAR, `N1` for NUMBER: the codes SYSTEM_TABLE records. (`type` is a keyword in Rust.)
    pub kind: String,
    /// Number of characters the column occupies in every row.
    pub width: usize,
}

/// `CREATE TABLE <table> (<column> <type>(<width>)[, ...])`: adds a table to SYSTEM_TABLE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTableStmt {
    /// The new table's name.
    pub table: String,
    /// Its columns, in file order.
    pub columns: Vec<ColumnDefinition>,
}

/// `DESCRIBE` lists the tables; `DESCRIBE <table>` lists that table's columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribeStmt {
    /// The table to describe, or `None` to list all tables.
    pub table: Option<String>,
}

/// Any parsed statement: exactly one of the statement types above.
///
/// [`Session::execute`](crate::session::Session::execute) uses a `match` to call the matching `run_...()` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// `SELECT ...`
    Select(SelectStmt),
    /// `INSERT INTO ...`
    Insert(InsertStmt),
    /// `UPDATE ...`
    Update(UpdateStmt),
    /// `DELETE FROM ...`
    Delete(DeleteStmt),
    /// `CREATE TABLE ...`
    CreateTable(CreateTableStmt),
    /// `COLUMN ... FORMAT ...`
    ColumnFormat(ColumnFormatStmt),
    /// `DESCRIBE [...]`
    Describe(DescribeStmt),
    /// `REM ...`, `-- ...`, or an empty line: nothing to run. (C++ needs an empty `CommentStmt` struct.)
    Comment,
}
