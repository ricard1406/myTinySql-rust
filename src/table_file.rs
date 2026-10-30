//! Reading, appending, editing and removing the rows of one table's data file.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::catalog::{Column, TableSchema};
use crate::util::{SqlError, SqlResult, is_blank, parse_integer, text_of, trim_right};

/// One row of a table: one value per column, trailing spaces removed.
pub type Row = Vec<String>;

/// Reads and changes the rows of one table's data file.
///
/// Each line of the file is one row; each column sits at the fixed positions given by the schema.
/// Changes that rewrite the file ([`remove_rows`](Self::remove_rows), [`update_rows`](Self::update_rows))
/// go through a temporary file, so a crash half-way never leaves a half-written table.
///
/// The `'a` is a *lifetime*: a `TableFile` borrows its schema, and the compiler checks that the
/// schema outlives it. C++ can only say so in a comment ("it must outlive this object").
pub struct TableFile<'a> {
    /// The table's definition.
    schema: &'a TableSchema,
    /// The data file.
    path: PathBuf,
}

/// What [`TableFile::rewrite`] does with one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Write the line back unchanged.
    Keep,
    /// Leave the line out.
    Remove,
    /// Write the line back as `decide` edited it.
    Update,
}

/// Checks that a value fits in its column; fails if it is longer than the column width.
fn check_fits(column: &Column, value: &str) -> SqlResult<()> {
    if value.len() > column.width() {
        return Err(SqlError::new(format!(
            "value '{value}' is too long for column {} (max {} characters)",
            column.name,
            column.width()
        )));
    }
    Ok(())
}

/// Writes a value into its column's characters of a row, padded with spaces to the column width.
///
/// The record must be at least as long as the row length, and the value checked with [`check_fits`]:
/// otherwise the slice below is out of bounds, and Rust stops the program (a *panic*) instead of
/// writing past the end.
fn place(record: &mut [u8], column: &Column, value: &str) {
    let field = &mut record[column.from..=column.to];
    field.fill(b' ');
    field[..value.len()].copy_from_slice(value.as_bytes());
}

/// The message for a data file that cannot be read.
fn not_found(path: &Path) -> SqlError {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    SqlError::new(format!("data file {name} not found"))
}

/// Tells whether a file exists, is not empty, and does not end with a line feed.
fn lacks_final_newline(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut last = [0u8; 1];
    // Seeking before the start of an empty file fails, which also means "no newline needed".
    file.seek(SeekFrom::End(-1)).is_ok() && file.read_exact(&mut last).is_ok() && last[0] != b'\n'
}

impl<'a> TableFile<'a> {
    /// Prepares access to one data file, usually from [`Catalog::data_file`](crate::catalog::Catalog::data_file).
    pub fn new(schema: &'a TableSchema, path: PathBuf) -> Self {
        TableFile { schema, path }
    }

    /// Reads the non-blank lines of the file, as bytes; fails if the file does not exist.
    fn read_lines(&self) -> SqlResult<Vec<Vec<u8>>> {
        let content = fs::read(&self.path).map_err(|_| not_found(&self.path))?;
        Ok(content.split(|&c| c == b'\n').filter(|line| !is_blank(line)).map(<[u8]>::to_vec).collect())
    }

    /// Reads every row of the file, each split into one value per column; blank lines are skipped.
    pub fn read_rows(&self) -> SqlResult<Vec<Row>> {
        Ok(self.read_lines()?.iter().map(|line| self.split_line(line)).collect())
    }

    /// Cuts one line of the file into its column values, trailing spaces removed.
    /// A line shorter than the row length gives empty values at the end.
    fn split_line(&self, line: &[u8]) -> Row {
        self.schema
            .columns
            .iter()
            .map(|column| {
                if column.from >= line.len() {
                    return String::new();
                }
                let end = (column.from + column.width()).min(line.len());
                trim_right(&text_of(&line[column.from..end])).to_string()
            })
            .collect()
    }

    /// Removes the rows for which `should_remove` returns true, and tells how many there were.
    ///
    /// `impl Fn(&Row) -> bool` takes any function or closure; unlike `std::function`, it is
    /// resolved at compile time, with no allocation or indirect call.
    pub fn remove_rows(&self, should_remove: impl Fn(&Row) -> bool) -> SqlResult<usize> {
        self.rewrite(|_, row| if should_remove(row) { Change::Remove } else { Change::Keep })
    }

    /// Changes some columns in the rows for which `should_update` returns true, and tells how many there were.
    ///
    /// `new_values` holds pairs of (column index, new value). Every value is checked against its column
    /// width before the file is touched, so a value too long changes nothing.
    pub fn update_rows(
        &self,
        should_update: impl Fn(&Row) -> bool,
        new_values: &[(usize, String)],
    ) -> SqlResult<usize> {
        for (index, value) in new_values {
            check_fits(&self.schema.columns[*index], value)?;
        }
        self.rewrite(|line, row| {
            if !should_update(row) {
                return Change::Keep;
            }
            if line.len() < self.schema.record_length {
                line.resize(self.schema.record_length, b' ');
            }
            for (index, value) in new_values {
                place(line, &self.schema.columns[*index], value);
            }
            Change::Update
        })
    }

    /// Rewrites the file through a temporary copy, line by line.
    ///
    /// `decide` is called for every line with the raw line (which it may edit in place) and the same
    /// line split into a [`Row`]; it returns what to do with that line. Returns how many lines were
    /// removed or edited; when 0, the file is left untouched.
    fn rewrite(&self, mut decide: impl FnMut(&mut Vec<u8>, &Row) -> Change) -> SqlResult<usize> {
        let mut kept = Vec::new();
        let mut changed = 0;
        for mut line in self.read_lines()? {
            let row = self.split_line(&line);
            let change = decide(&mut line, &row);
            if change != Change::Keep {
                changed += 1;
            }
            if change != Change::Remove {
                kept.extend_from_slice(&line);
                kept.push(b'\n');
            }
        }

        if changed == 0 {
            return Ok(0); // nothing to rewrite
        }

        let mut temporary = self.path.clone().into_os_string();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        // `fs::write` opens, writes and closes the file: it is closed before it replaces the original.
        fs::write(&temporary, &kept).map_err(|_| SqlError::new(format!("cannot write {}", temporary.display())))?;
        fs::rename(&temporary, &self.path)
            .map_err(|e| SqlError::new(format!("cannot replace {}: {e}", self.path.display())))?;
        Ok(changed)
    }

    /// The ID to give the next new row: the highest numeric ID in the file plus one,
    /// or 1 for an empty or missing file.
    pub fn next_id(&self) -> SqlResult<i64> {
        if !self.path.exists() {
            return Ok(1);
        }
        let max_id =
            self.read_rows()?.iter().filter_map(|row| row.first().and_then(|id| parse_integer(id))).fold(0, i64::max);
        Ok(max_id + 1)
    }

    /// Adds one row at the end of the file: one value per column, ID included.
    ///
    /// Fails if the number of values is wrong, a value is too long for its column,
    /// or the file cannot be written.
    pub fn append(&self, values: Row) -> SqlResult<()> {
        self.append_rows(&[values])
    }

    /// Adds several rows at the end of the file, in one write. Every row is checked before the
    /// file is touched, so a bad row adds nothing.
    pub fn append_rows(&self, rows: &[Row]) -> SqlResult<()> {
        // Build every record first, so a bad value stops before the file is touched.
        let mut records = Vec::new();
        for values in rows {
            if values.len() != self.schema.columns.len() {
                return Err(SqlError::new(format!(
                    "table {} has {} columns, got {} values",
                    self.schema.name,
                    self.schema.columns.len(),
                    values.len()
                )));
            }
            let mut record = vec![b' '; self.schema.record_length];
            for (column, value) in self.schema.columns.iter().zip(values) {
                check_fits(column, value)?;
                place(&mut record, column, value);
            }
            records.push(record);
        }

        // Make sure the new row starts on its own line, even if the file lacks a final newline.
        let mut text = Vec::new();
        if lacks_final_newline(&self.path) {
            text.push(b'\n');
        }
        for record in records {
            text.extend_from_slice(&record);
            text.push(b'\n');
        }

        let cannot_write = || SqlError::new(format!("cannot write {}", self.path.display()));
        let mut out = OpenOptions::new().append(true).create(true).open(&self.path).map_err(|_| cannot_write())?;
        out.write_all(&text).map_err(|_| cannot_write())
    }
}
