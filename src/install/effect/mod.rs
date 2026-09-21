//! Changing the world.
//!
//! Everything under here writes, deletes, or runs something. Its counterpart
//! is [`super::facts`], which reads; between the two sits the part of the
//! install that only decides, and touches nothing.

mod apply;
mod nvram;
mod run;
mod sync;

pub(crate) use apply::apply;
pub(crate) use nvram::register;
pub(crate) use run::{CmdError, Output, run};
pub(crate) use sync::sync;
