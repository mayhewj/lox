#[macro_use]
extern crate log;
extern crate time;

pub mod callable;
pub mod class;
pub mod environment;
pub mod instance;
pub mod interpreter;
pub mod parser;
pub mod primitive;
pub mod resolver;
pub mod scanner;

use std::error::Error;
use std::result;

pub type Result<T> = result::Result<T, Box<Error>>;
