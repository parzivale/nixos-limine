//! A program and its arguments, decided without running anything.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// Working out what to run is a function of the config and the facts;
/// running it is [`crate::install::effect::run`]'s job, which is what makes
/// the deciding testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    program: PathBuf,
    args: Vec<OsString>,
}

impl Invocation {
    pub(crate) fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    #[must_use]
    pub(crate) fn args<I, A>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    pub(crate) fn program(&self) -> &Path {
        &self.program
    }

    pub(crate) fn arguments(&self) -> &[OsString] {
        &self.args
    }

    /// `program arg arg`, for tests that assert on what would be run.
    #[cfg(test)]
    pub(crate) fn line(&self) -> String {
        let mut line = self.program.display().to_string();

        for arg in &self.args {
            line.push(' ');
            line.push_str(&arg.to_string_lossy());
        }

        line
    }
}
