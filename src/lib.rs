//! myTinySQL: a tiny SQL interpreter over fixed-width text files, written in Rust as a learning project.
//!
//! It is a line-by-line port of the C++20 version (<https://github.com/ricard1406/myTinySql>):
//! every C++ file has a Rust module with the same job.
//!
//! | C++                              | Rust            | Job                                              |
//! | -------------------------------- | --------------- | ------------------------------------------------ |
//! | `Util.hpp` / `.cpp`              | [`util`]        | `SqlError` and string helpers                    |
//! | `Statement.hpp`                  | [`statement`]   | The parsed form of every statement               |
//! | `Parser.cpp`                     | [`parser`]      | Tokenizer, parser, and splitting a script        |
//! | `Catalog.hpp` / `.cpp`           | [`catalog`]     | Reading `SYSTEM_TABLE`                           |
//! | `TableFile.hpp` / `.cpp`         | [`table_file`]  | Reading and changing one table's data file       |
//! | `Session.hpp` / `.cpp`           | [`session`]     | Running statements                               |
//! | `Output.hpp` / `.cpp`            | [`output`]      | Console text and JSON                            |
//! | `Server.hpp` / `.cpp`            | [`server`]      | The browser server                               |
//! | `main.cpp`                       | `main.rs`       | The command line                                 |
//!
//! Every input becomes SQL text. [`parser::split_statements`] cuts it into statements,
//! [`parser::parse_statement`] turns each one into a [`statement::Statement`], and
//! [`session::Session::execute`] runs it.

// Like Doxygen's "undocumented" warnings in the C++ version: every public item needs a `///` comment.
#![warn(missing_docs)]
// No `unsafe` blocks anywhere: the compiler refuses them, so memory safety is checked at compile time.
#![forbid(unsafe_code)]

pub mod catalog;
pub mod output;
pub mod parser;
pub mod server;
pub mod session;
pub mod statement;
pub mod table_file;
pub mod util;
