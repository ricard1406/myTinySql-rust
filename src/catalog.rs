//! The table definitions read from `SYSTEM_TABLE`: tables, their columns and positions.

use std::fs;
use std::path::{Path, PathBuf};

use crate::util::{SqlError, SqlResult, is_blank, parse_integer, text_of, trim};

/// Name of the catalogue table: it describes every table, itself included, and is its own data file.
pub const CATALOG_TABLE: &str = "SYSTEM_TABLE";

/// One column of a table: the characters it occupies in every fixed-width row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Column name, e.g. `NOME`.
    pub name: String,
    /// `N1` or `S1`: recorded in SYSTEM_TABLE, not enforced. (`type` is a keyword in Rust.)
    pub kind: String,
    /// First character of the column in a row, counted from 0.
    pub from: usize,
    /// Last character of the column, inclusive.
    pub to: usize,
}

impl Column {
    /// Number of characters the column occupies: `to - from + 1`.
    pub fn width(&self) -> usize {
        self.to - self.from + 1
    }
}

/// The definition of one table: its name, its columns, and the length of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    /// Table name, e.g. `EMPLOY`.
    pub name: String,
    /// Columns in file order.
    pub columns: Vec<Column>,
    /// Length of a full row, in characters.
    pub record_length: usize,
}

impl TableSchema {
    /// Finds a column by name (case-insensitive): its position in `columns`, or `None`.
    pub fn index_of(&self, column_name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(column_name))
    }

    /// Tells whether rows are numbered automatically: true when the first column is called ID,
    /// and INSERT then fills it in.
    pub fn has_auto_id(&self) -> bool {
        self.columns.first().is_some_and(|c| c.name.eq_ignore_ascii_case("ID"))
    }
}

/// The position of one field in a SYSTEM_TABLE line.
///
/// SYSTEM_TABLE describes itself in its first lines; the constants below are the positions of its own fields.
#[derive(Clone, Copy)]
struct Field {
    /// First character, counted from 0.
    from: usize,
    /// Last character, inclusive.
    to: usize,
}

/// Table name.
const TABLE: Field = Field { from: 15, to: 34 };
/// Column name.
const NAME: Field = Field { from: 35, to: 54 };
/// Column type (`N1` or `S1`).
const TYPE: Field = Field { from: 55, to: 59 };
/// First character of the column in a data row.
const FROM: Field = Field { from: 60, to: 64 };
/// Last character of the column.
const TO: Field = Field { from: 65, to: 69 };
/// Length of a full data row.
const RECORD_LENGTH: Field = Field { from: 75, to: 79 };

/// Cuts one field out of a SYSTEM_TABLE line, without surrounding spaces; empty if the line is too short.
///
/// The line is bytes (`&[u8]`), because a Rust `&str` cannot be cut in the middle of a UTF-8 character
/// and the positions are counted in bytes, as in C++.
fn cut(line: &[u8], field: Field) -> String {
    if field.from >= line.len() {
        return String::new();
    }
    let end = (field.to + 1).min(line.len());
    trim(&text_of(&line[field.from..end])).to_string()
}

/// Reads one numeric field of a SYSTEM_TABLE line; fails if it is not a non-negative integer.
fn number(line: &[u8], field: Field, line_number: usize) -> SqlResult<usize> {
    parse_integer(&cut(line, field)).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| {
        SqlError::new(format!(
            "{CATALOG_TABLE} line {line_number}: expected a number in characters {}-{}",
            field.from, field.to
        ))
    })
}

/// All table definitions, read from the `SYSTEM_TABLE` file (one 80-character line per column).
#[derive(Debug, Clone)]
pub struct Catalog {
    /// Where the files are.
    data_dir: PathBuf,
    /// Every table, in file order.
    tables: Vec<TableSchema>,
}

impl Catalog {
    /// Reads `SYSTEM_TABLE` from a data directory.
    ///
    /// Fails if the file cannot be opened or a line is malformed.
    pub fn load(data_dir: &Path) -> SqlResult<Catalog> {
        let path = data_dir.join(CATALOG_TABLE);
        let content =
            fs::read(&path).map_err(|_| SqlError::new(format!("cannot open the catalogue {}", path.display())))?;

        let mut tables: Vec<TableSchema> = Vec::new();
        for (index, line) in content.split(|&c| c == b'\n').enumerate() {
            let line_number = index + 1;
            if is_blank(line) {
                continue;
            }
            let table_name = cut(line, TABLE);
            let column = Column {
                name: cut(line, NAME),
                kind: cut(line, TYPE),
                from: number(line, FROM, line_number)?,
                to: number(line, TO, line_number)?,
            };
            if table_name.is_empty() || column.name.is_empty() || column.to < column.from {
                return Err(SqlError::new(format!("{CATALOG_TABLE} line {line_number}: invalid column definition")));
            }
            let record_length = number(line, RECORD_LENGTH, line_number)?.max(column.to + 1);

            // Find the table, or add it at the end. An index, not a reference: a reference into
            // `tables` cannot be kept across `push`, which may move the elements (the borrow checker
            // refuses it; in C++ an iterator kept across `push_back` compiles, and dangles).
            let index = match tables.iter().position(|t| t.name.eq_ignore_ascii_case(&table_name)) {
                Some(index) => index,
                None => {
                    tables.push(TableSchema { name: table_name, columns: Vec::new(), record_length: 0 });
                    tables.len() - 1
                }
            };
            let table = &mut tables[index];
            table.columns.push(column);
            table.record_length = table.record_length.max(record_length);
        }
        Ok(Catalog { data_dir: data_dir.to_path_buf(), tables })
    }

    /// Finds a table by name (case-insensitive); fails if there is no such table.
    ///
    /// The `&TableSchema` it returns borrows from the catalogue: the compiler makes sure the
    /// catalogue lives at least as long as the reference.
    pub fn table(&self, name: &str) -> SqlResult<&TableSchema> {
        self.tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| SqlError::new(format!("unknown table '{name}'")))
    }

    /// All tables, in the order they appear in `SYSTEM_TABLE`.
    pub fn tables(&self) -> &[TableSchema] {
        &self.tables
    }

    /// The file holding a table's rows: `SYSTEM_TABLE` for the catalogue itself, `<name>_DATA` for every other table.
    pub fn data_file(&self, table: &TableSchema) -> PathBuf {
        if table.name.eq_ignore_ascii_case(CATALOG_TABLE) {
            return self.data_dir.join(CATALOG_TABLE);
        }
        self.data_dir.join(format!("{}_DATA", table.name))
    }
}
