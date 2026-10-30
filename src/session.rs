//! Running statements, and the results they produce.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::PathBuf;

use crate::catalog::{CATALOG_TABLE, Catalog, Column, TableSchema};
use crate::parser::{parse_statement, split_statements};
use crate::statement::{
    ColumnFormatStmt, Condition, CreateTableStmt, DeleteStmt, DescribeStmt, InsertStmt, Operator, SelectItem,
    SelectStmt, Statement, UpdateStmt,
};
use crate::table_file::{Row, TableFile};
use crate::util::{SqlError, SqlResult, matches_like, parse_integer, trim};

/// The rows produced by a SELECT or DESCRIBE. All per-column vectors have the same length.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultSet {
    /// Column titles.
    pub headers: Vec<String>,
    /// Display width of each column in console output.
    pub widths: Vec<usize>,
    /// True for a `'literal'` SELECT item.
    pub literal: Vec<bool>,
    /// Width of each column in the data file (0 for a literal).
    pub storage_widths: Vec<usize>,
    /// The values, one inner vector per row.
    pub rows: Vec<Vec<String>>,
    /// Output column holding the row ID, if rows can be edited and deleted by ID.
    pub id_column: Option<usize>,
}

/// What running one statement produced: a table, a message, or an error.
///
/// `#[derive(Default)]` gives every field its empty value, so `StatementResult { failed: true, ..Default::default() }`
/// fills in the rest, like the default member values in the C++ struct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatementResult {
    /// The statement as it was run.
    pub sql: String,
    /// The table the statement worked on, if any.
    pub table_name: String,
    /// The rows, for SELECT and DESCRIBE.
    pub table: Option<ResultSet>,
    /// E.g. "1 row inserted ...", or the error text.
    pub message: String,
    /// True if the statement failed; `message` holds the error.
    pub failed: bool,
    /// True for COLUMN and comments: nothing to show.
    pub silent: bool,
}

impl StatementResult {
    /// A result with a message, about one table.
    fn message(table_name: &str, message: String) -> Self {
        StatementResult { table_name: table_name.to_string(), message, ..Default::default() }
    }

    /// A result with nothing to show (COLUMN and comments).
    fn silent() -> Self {
        StatementResult { silent: true, ..Default::default() }
    }
}

/// Applies a WHERE test to one value.
///
/// `LIKE` matches a pattern, ignoring case (see [`matches_like`]). The other operators compare numbers
/// as numbers when both sides are integers, otherwise as case-sensitive text.
fn matches(field_value: &str, condition: &Condition) -> bool {
    let order = match (parse_integer(field_value), parse_integer(&condition.value)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => trim(field_value).cmp(trim(&condition.value)),
    };
    match condition.op {
        Operator::Like => matches_like(field_value, &condition.value),
        Operator::Equal => order == Ordering::Equal,
        Operator::NotEqual => order != Ordering::Equal,
        Operator::Less => order == Ordering::Less,
        Operator::LessOrEqual => order != Ordering::Greater,
        Operator::Greater => order == Ordering::Greater,
        Operator::GreaterOrEqual => order != Ordering::Less,
    }
}

/// Builds a result whose column widths fit the headers and the values (used by DESCRIBE).
fn fitted(headers: &[&str], rows: Vec<Vec<String>>) -> ResultSet {
    let widths =
        (0..headers.len()).map(|i| rows.iter().map(|row| row[i].len()).fold(headers[i].len(), usize::max)).collect();
    ResultSet {
        headers: headers.iter().map(ToString::to_string).collect(),
        widths,
        literal: vec![false; headers.len()],
        storage_widths: vec![0; headers.len()],
        rows,
        id_column: None,
    }
}

/// Writes a count with a noun in singular or plural, e.g. "1 row" or "3 rows".
fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The message for a column name that the table does not have.
fn unknown_column(column: &str, schema: &TableSchema) -> SqlError {
    SqlError::new(format!("unknown column '{column}' in table {}", schema.name))
}

/// Longest table or column name: the width of the TABLE and FIELD columns of SYSTEM_TABLE.
const MAX_NAME_LENGTH: usize = 20;

/// Longest row: SYSTEM_TABLE keeps column positions in 5 characters.
const MAX_RECORD_LENGTH: usize = 99999;

/// Checks a new table or column name; `what` is "table" or "column", for the error message.
///
/// The name must start with a letter, have only letters, digits, `_` and `-`,
/// and be at most [`MAX_NAME_LENGTH`] characters long.
fn check_name(name: &str, what: &str) -> SqlResult<()> {
    let allowed = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_NAME_LENGTH
        || !bytes[0].is_ascii_alphabetic()
        || !bytes.iter().all(|&c| allowed(c))
    {
        return Err(SqlError::new(format!(
            "invalid {what} name '{name}': use up to {MAX_NAME_LENGTH} letters, digits, _ and -, starting with a letter"
        )));
    }
    Ok(())
}

/// Runs statements against the tables in one data directory.
///
/// `COLUMN ... FORMAT` settings last for the whole session. Every statement reloads the catalogue,
/// so changes to the files are seen at once.
pub struct Session {
    /// Where the tables are.
    data_dir: PathBuf,
    /// Upper-case column name to display width.
    display_widths: HashMap<String, usize>,
}

impl Session {
    /// Starts a session on the directory holding `SYSTEM_TABLE` and the data files.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Session { data_dir: data_dir.into(), display_widths: HashMap::new() }
    }

    /// Parses and runs one statement.
    ///
    /// Errors never escape: a failed statement gives a result with `failed` set and the error in `message`.
    pub fn execute(&mut self, sql: &str) -> StatementResult {
        // `and_then` runs the statement only if it parsed; either step's `Err` lands in the second arm.
        let mut result = match parse_statement(sql).and_then(|statement| self.run(statement)) {
            Ok(result) => result,
            Err(error) => StatementResult { failed: true, message: error.to_string(), ..Default::default() },
        };
        result.sql = trim(sql).to_string();
        result
    }

    /// Runs every statement of a script (one per line, or separated by `;`), in order.
    ///
    /// Returns one result per statement; a failed statement does not stop the ones after it.
    pub fn execute_script(&mut self, script: &str) -> Vec<StatementResult> {
        split_statements(script).iter().map(|sql| self.execute(sql)).collect()
    }

    /// Calls the `run_...()` method for the kind of statement. (C++: `std::visit` and overloads of `run()`.)
    ///
    /// A `match` must name every variant: adding a statement type without a line here does not compile.
    fn run(&mut self, statement: Statement) -> SqlResult<StatementResult> {
        match statement {
            Statement::Select(stmt) => self.run_select(&stmt),
            Statement::Insert(stmt) => self.run_insert(&stmt),
            Statement::Update(stmt) => self.run_update(&stmt),
            Statement::Delete(stmt) => self.run_delete(&stmt),
            Statement::CreateTable(stmt) => self.run_create_table(&stmt),
            Statement::ColumnFormat(stmt) => Ok(self.run_column_format(&stmt)),
            Statement::Describe(stmt) => self.run_describe(&stmt),
            Statement::Comment => Ok(StatementResult::silent()),
        }
    }

    /// Runs a SELECT; fails on an unknown table or column, or a missing data file.
    fn run_select(&self, stmt: &SelectStmt) -> SqlResult<StatementResult> {
        let catalog = Catalog::load(&self.data_dir)?;
        let schema = catalog.table(&stmt.table)?;

        // What each output column shows: a table column (by index) or a literal text.
        struct Output {
            header: String,
            source: Option<usize>,
            width: usize,
        }
        let mut outputs = Vec::new();
        let add_column = |outputs: &mut Vec<Output>, index: usize| {
            let column = &schema.columns[index];
            // COLUMN ... FORMAT wins; otherwise the column is wide enough for its values and its name.
            let width = match self.display_widths.get(&column.name.to_ascii_uppercase()) {
                Some(&custom) => custom,
                None => column.width().max(column.name.len()),
            };
            outputs.push(Output { header: column.name.clone(), source: Some(index), width });
        };
        for item in &stmt.items {
            match item {
                SelectItem::Star => (0..schema.columns.len()).for_each(|i| add_column(&mut outputs, i)),
                SelectItem::Column(name) => {
                    let index = schema.index_of(name).ok_or_else(|| unknown_column(name, schema))?;
                    add_column(&mut outputs, index);
                }
                SelectItem::Literal(text) => {
                    outputs.push(Output { header: text.clone(), source: None, width: text.len() })
                }
            }
        }

        let where_index = match &stmt.condition {
            Some(condition) => {
                Some(schema.index_of(&condition.column).ok_or_else(|| unknown_column(&condition.column, schema))?)
            }
            None => None,
        };

        let mut result = ResultSet {
            headers: outputs.iter().map(|o| o.header.clone()).collect(),
            widths: outputs.iter().map(|o| o.width).collect(),
            literal: outputs.iter().map(|o| o.source.is_none()).collect(),
            storage_widths: outputs.iter().map(|o| o.source.map_or(0, |i| schema.columns[i].width())).collect(),
            ..Default::default()
        };

        // Rows can be edited and deleted by ID when the table has an ID column, the result shows it,
        // and the table is not the catalogue itself.
        if schema.has_auto_id() && !schema.name.eq_ignore_ascii_case(CATALOG_TABLE) {
            result.id_column = outputs.iter().position(|o| o.source == Some(0));
        }

        for row in TableFile::new(schema, catalog.data_file(schema)).read_rows()? {
            if let (Some(index), Some(condition)) = (where_index, &stmt.condition)
                && !matches(&row[index], condition)
            {
                continue;
            }
            let values = outputs
                .iter()
                .map(|output| match output.source {
                    Some(index) => row[index].clone(),
                    None => output.header.clone(),
                })
                .collect();
            result.rows.push(values);
        }

        Ok(StatementResult {
            table_name: schema.name.clone(),
            message: plural(result.rows.len(), "row"),
            table: Some(result),
            ..Default::default()
        })
    }

    /// Runs an INSERT, assigning the next ID when it is left out.
    ///
    /// Fails on a wrong number of values, a duplicate ID, or a value too long for its column.
    fn run_insert(&self, stmt: &InsertStmt) -> SqlResult<StatementResult> {
        let catalog = Catalog::load(&self.data_dir)?;
        let schema = catalog.table(&stmt.table)?;
        let file = TableFile::new(schema, catalog.data_file(schema));

        let mut values: Row = stmt.values.clone();
        let columns = schema.columns.len();
        if schema.has_auto_id() && values.len() + 1 == columns {
            values.insert(0, file.next_id()?.to_string());
        } else if values.len() != columns {
            let hint = if schema.has_auto_id() {
                format!(" (give {} and ID is assigned)", columns - 1)
            } else {
                String::new()
            };
            return Err(SqlError::new(format!(
                "table {} has {columns} columns{hint}: got {} values",
                schema.name,
                values.len()
            )));
        } else if schema.has_auto_id() {
            let rows = if catalog.data_file(schema).exists() { file.read_rows()? } else { Vec::new() };
            if rows.iter().any(|row| row[0] == values[0]) {
                return Err(SqlError::new(format!("table {} already has a row with ID {}", schema.name, values[0])));
            }
        }
        let id = values[0].clone();
        file.append(values)?;

        let mut message = format!("1 row inserted into {}", schema.name);
        if schema.has_auto_id() {
            message += &format!(" with ID {id}");
        }
        Ok(StatementResult::message(&schema.name, message))
    }

    /// Runs an UPDATE.
    ///
    /// Fails on an unknown column, an attempt to change the ID or SYSTEM_TABLE, or a value too long for its column.
    fn run_update(&self, stmt: &UpdateStmt) -> SqlResult<StatementResult> {
        let catalog = Catalog::load(&self.data_dir)?;
        let schema = catalog.table(&stmt.table)?;
        if schema.name.eq_ignore_ascii_case(CATALOG_TABLE) {
            return Err(SqlError::new("SYSTEM_TABLE cannot be changed with UPDATE: edit the file instead"));
        }

        let mut new_values = Vec::new();
        for assignment in &stmt.assignments {
            let index =
                schema.index_of(&assignment.column).ok_or_else(|| unknown_column(&assignment.column, schema))?;
            if index == 0 && schema.has_auto_id() {
                return Err(SqlError::new("the ID column cannot be changed: rows are found by their ID"));
            }
            new_values.push((index, assignment.value.clone()));
        }
        let where_index =
            schema.index_of(&stmt.condition.column).ok_or_else(|| unknown_column(&stmt.condition.column, schema))?;

        let updated = TableFile::new(schema, catalog.data_file(schema))
            .update_rows(|row| matches(&row[where_index], &stmt.condition), &new_values)?;

        Ok(StatementResult::message(&schema.name, format!("{} updated in {}", plural(updated, "row"), schema.name)))
    }

    /// Runs a DELETE; fails on an unknown column or an attempt to change SYSTEM_TABLE.
    fn run_delete(&self, stmt: &DeleteStmt) -> SqlResult<StatementResult> {
        let catalog = Catalog::load(&self.data_dir)?;
        let schema = catalog.table(&stmt.table)?;
        if schema.name.eq_ignore_ascii_case(CATALOG_TABLE) {
            return Err(SqlError::new("SYSTEM_TABLE cannot be changed with DELETE: edit the file instead"));
        }
        let where_index =
            schema.index_of(&stmt.condition.column).ok_or_else(|| unknown_column(&stmt.condition.column, schema))?;

        let removed = TableFile::new(schema, catalog.data_file(schema))
            .remove_rows(|row| matches(&row[where_index], &stmt.condition))?;

        Ok(StatementResult::message(&schema.name, format!("{} deleted from {}", plural(removed, "row"), schema.name)))
    }

    /// Runs a CREATE TABLE: adds one SYSTEM_TABLE line per column and an empty data file.
    ///
    /// Fails on an invalid or duplicate name, an ID column that is not the first one,
    /// a row too long for SYSTEM_TABLE, or a data file that already exists.
    fn run_create_table(&self, stmt: &CreateTableStmt) -> SqlResult<StatementResult> {
        let catalog = Catalog::load(&self.data_dir)?;

        // The new table's definition; names are kept in upper case, like the existing tables.
        check_name(&stmt.table, "table")?;
        let mut schema = TableSchema { name: stmt.table.to_ascii_uppercase(), columns: Vec::new(), record_length: 0 };
        if catalog.tables().iter().any(|t| t.name.eq_ignore_ascii_case(&schema.name)) {
            return Err(SqlError::new(format!("table {} already exists", schema.name)));
        }
        for definition in &stmt.columns {
            check_name(&definition.name, "column")?;
            let name = definition.name.to_ascii_uppercase();
            if schema.index_of(&name).is_some() {
                return Err(SqlError::new(format!("column {name} appears twice")));
            }
            if name == "ID" && !schema.columns.is_empty() {
                return Err(SqlError::new("the ID column must be the first one: rows are numbered by it"));
            }
            // `saturating_add` stops at the largest usize instead of overflowing: a debug build of
            // Rust stops the program on an overflow, and C++ would silently wrap around to a small number.
            let end = schema.record_length.saturating_add(definition.width);
            schema.columns.push(Column {
                name,
                kind: definition.kind.clone(),
                from: schema.record_length,
                to: end - 1,
            });
            schema.record_length = end;
        }
        if schema.record_length > MAX_RECORD_LENGTH {
            return Err(SqlError::new(format!(
                "a row of table {} would be {} characters long: the limit is {MAX_RECORD_LENGTH}",
                schema.name, schema.record_length
            )));
        }

        // One SYSTEM_TABLE line per column, numbered after the last line.
        let catalog_schema = catalog.table(CATALOG_TABLE)?;
        let catalog_file = TableFile::new(catalog_schema, catalog.data_file(catalog_schema));
        let first_id = catalog_file.next_id()?;
        let lines: Vec<Row> = (1..)
            .zip(&schema.columns)
            .map(|(number, column)| {
                vec![
                    (first_id + number - 1).to_string(),
                    number.to_string(),
                    schema.name.clone(),
                    column.name.clone(),
                    column.kind.clone(),
                    column.from.to_string(),
                    column.to.to_string(),
                    column.width().to_string(),
                    schema.record_length.to_string(),
                ]
            })
            .collect();

        // First the empty data file, then the catalogue: if the catalogue cannot be written,
        // the data file is removed again and nothing has changed. `create_new` fails if the file
        // already exists, in the same system call that creates it.
        let data_file = catalog.data_file(&schema);
        if let Err(error) = OpenOptions::new().write(true).create_new(true).open(&data_file) {
            let name = data_file.file_name().unwrap_or_default().to_string_lossy();
            return Err(SqlError::new(if error.kind() == ErrorKind::AlreadyExists {
                format!("data file {name} already exists: remove it, or choose another name")
            } else {
                format!("cannot create {}", data_file.display())
            }));
        }
        if let Err(error) = catalog_file.append_rows(&lines) {
            let _ = fs::remove_file(&data_file);
            return Err(error);
        }

        Ok(StatementResult::message(
            &schema.name,
            format!(
                "table {} created with {} (row length {})",
                schema.name,
                plural(schema.columns.len(), "column"),
                schema.record_length
            ),
        ))
    }

    /// Remembers a column's display width for the rest of the session.
    fn run_column_format(&mut self, stmt: &ColumnFormatStmt) -> StatementResult {
        self.display_widths.insert(stmt.column.to_ascii_uppercase(), stmt.width);
        StatementResult::silent()
    }

    /// Lists the tables, or one table's columns; fails on an unknown table.
    fn run_describe(&self, stmt: &DescribeStmt) -> SqlResult<StatementResult> {
        let catalog = Catalog::load(&self.data_dir)?;

        let Some(table) = &stmt.table else {
            let rows: Vec<Vec<String>> = catalog
                .tables()
                .iter()
                .map(|t| vec![t.name.clone(), t.columns.len().to_string(), t.record_length.to_string()])
                .collect();
            return Ok(StatementResult {
                message: plural(rows.len(), "table"),
                table: Some(fitted(&["TABLE", "COLUMNS", "ROW_LENGTH"], rows)),
                ..Default::default()
            });
        };

        let schema = catalog.table(table)?;
        let rows: Vec<Vec<String>> = schema
            .columns
            .iter()
            .map(|c| vec![c.name.clone(), c.kind.clone(), c.from.to_string(), c.to.to_string(), c.width().to_string()])
            .collect();
        Ok(StatementResult {
            table_name: schema.name.clone(),
            message: plural(rows.len(), "column"),
            table: Some(fitted(&["COLUMN", "TYPE", "FROM", "TO", "WIDTH"], rows)),
            ..Default::default()
        })
    }
}
