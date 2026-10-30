//! Console text, and the JSON answers of the browser server's API.
//!
//! The JSON is written by hand, as in C++: with a crate such as `serde_json` it would be a few
//! `#[derive(Serialize)]` lines, but this version uses only the standard library.

use crate::catalog::{CATALOG_TABLE, TableSchema};
use crate::session::{ResultSet, StatementResult};
use crate::util::trim_right;

/// Cuts or pads text to an exact width, in bytes, as the C++ version counts.
///
/// `filler` pads (space for values, dash for headers). A cut never splits a UTF-8 character: Rust
/// does not allow a `&str` that is not valid UTF-8, so the cut moves back to the start of the
/// character and one more filler is added. (C++ would print half a character.)
fn fit(text: &str, width: usize, filler: char) -> String {
    let mut end = text.len().min(width);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = text[..end].to_string();
    result.extend(std::iter::repeat_n(filler, width - end));
    result
}

/// Builds the dashed header line of a console table, e.g. `ID-------- NOME----`, columns separated by one space.
fn header_line(table: &ResultSet) -> String {
    let headers: Vec<String> = table.headers.iter().zip(&table.widths).map(|(h, &w)| fit(h, w, '-')).collect();
    headers.join(" ")
}

/// Writes text as a JSON string: in double quotes, with quotes, backslashes and control characters escaped.
///
/// For example, the text `say "hi"` becomes `"say \"hi\""`. Other characters (all of UTF-8) are passed through.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)), // other control characters
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Formats one result for the console.
///
/// An echo of the statement, then a dashed header line and fixed-width rows, a message, or an error;
/// empty for a silent result.
pub fn render_text(result: &StatementResult) -> String {
    if result.silent {
        return String::new();
    }
    let mut out = format!("> {}\n", result.sql);
    if result.failed {
        return out + &format!("ERROR: {}\n\n", result.message);
    }
    match &result.table {
        Some(table) => {
            out += trim_right(&header_line(table));
            out.push('\n');
            for row in &table.rows {
                let cells: Vec<String> = row.iter().zip(&table.widths).map(|(value, &w)| fit(value, w, ' ')).collect();
                out += trim_right(&cells.join(" "));
                out.push('\n');
            }
            out += &format!("({})\n\n", result.message);
        }
        None => out += &format!("{}\n\n", result.message),
    }
    out
}

/// Builds the JSON answer of `POST /api/sql`: one object per statement that has something to show.
///
/// Example: `{"results":[{"sql":"DELETE FROM EMPLOY WHERE ID = 3","ok":true,"message":"1 row deleted from EMPLOY",
/// "table":"EMPLOY"}]}`. A SELECT or DESCRIBE also has `"columns"` (name, width in the data file, and whether
/// the browser may edit the cell), `"rows"` (arrays of strings) and `"idColumn"` (index of the ID, or `null`).
/// Silent results (COLUMN, comments) are left out.
pub fn render_json(results: &[StatementResult]) -> String {
    let objects: Vec<String> = results
        .iter()
        .filter(|result| !result.silent) // COLUMN and comments have nothing to show
        .map(|result| {
            let mut out = format!(
                "{{\"sql\":{},\"ok\":{},\"message\":{},\"table\":{}",
                json_string(&result.sql),
                !result.failed,
                json_string(&result.message),
                json_string(&result.table_name)
            );
            if let Some(table) = &result.table {
                let columns: Vec<String> = (0..table.headers.len())
                    .map(|i| {
                        // A cell can be edited when the rows have IDs, and it is a real column other than the ID.
                        let editable = table.id_column.is_some_and(|id| id != i) && !table.literal[i];
                        format!(
                            "{{\"name\":{},\"width\":{},\"editable\":{editable}}}",
                            json_string(&table.headers[i]),
                            table.storage_widths[i]
                        )
                    })
                    .collect();
                let rows: Vec<String> = table
                    .rows
                    .iter()
                    .map(|row| format!("[{}]", row.iter().map(|v| json_string(v)).collect::<Vec<_>>().join(",")))
                    .collect();
                let id_column = table.id_column.map_or("null".to_string(), |id| id.to_string());
                out += &format!(
                    ",\"columns\":[{}],\"rows\":[{}],\"idColumn\":{id_column}",
                    columns.join(","),
                    rows.join(",")
                );
            }
            out + "}"
        })
        .collect();
    format!("{{\"results\":[{}]}}", objects.join(","))
}

/// Builds the JSON answer of `GET /api/tables`: every table with its columns.
///
/// Example: `{"tables":[{"name":"EMPLOY","readOnly":false,"autoId":true,"columns":[{"name":"ID","width":10},...]}]}`.
/// `readOnly` is true for SYSTEM_TABLE, which the page must not change.
pub fn render_tables_json(tables: &[TableSchema]) -> String {
    let objects: Vec<String> = tables
        .iter()
        .map(|table| {
            let columns: Vec<String> = table
                .columns
                .iter()
                .map(|c| format!("{{\"name\":{},\"width\":{}}}", json_string(&c.name), c.width()))
                .collect();
            format!(
                "{{\"name\":{},\"readOnly\":{},\"autoId\":{},\"columns\":[{}]}}",
                json_string(&table.name),
                table.name.eq_ignore_ascii_case(CATALOG_TABLE),
                table.has_auto_id(),
                columns.join(",")
            )
        })
        .collect();
    format!("{{\"tables\":[{}]}}", objects.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_pads_and_cuts() {
        assert_eq!(fit("ID", 5, '-'), "ID---");
        assert_eq!(fit("Torino", 3, ' '), "Tor");
        assert_eq!(fit("Città", 4, ' '), "Citt"); // "à" is 2 bytes and would not fit
        assert_eq!(fit("Città", 5, ' '), "Citt "); // the cut moves back, a space fills in
    }

    #[test]
    fn json_escapes() {
        assert_eq!(json_string("say \"hi\"\n"), r#""say \"hi\"\n""#);
        assert_eq!(json_string("\u{1}é"), "\"\\u0001é\"");
    }
}
