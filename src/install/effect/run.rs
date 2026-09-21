//! Running the helper binaries the install shells out to.

use crate::util::cmd::Invocation;
use snafu::{ResultExt as _, Snafu, ensure};
use std::{
    path::PathBuf,
    process::{Command, ExitStatus},
};

/// A finished run of a helper binary.
#[derive(Debug)]
pub(crate) struct Output {
    program: PathBuf,
    status: ExitStatus,
    text: String,
}

impl Output {
    pub(crate) const fn status(&self) -> ExitStatus {
        self.status
    }

    /// What the program printed, stdout followed by stderr.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Turn a non-zero exit into an error, for callers that treat one as fatal.
    pub(crate) fn success(self) -> Result<(), CmdError> {
        ensure!(
            self.status.success(),
            ExitSnafu {
                program: self.program,
                status: self.status,
                text: self.text,
            }
        );

        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum CmdError {
    /// The binary could not be started at all.
    #[snafu(display("could not run {}", program.display()))]
    Spawn {
        source: std::io::Error,
        program: PathBuf,
    },
    /// It ran and failed, taking whatever it had to say with it.
    #[snafu(display("{} failed: {status}{}", program.display(), Text(text)))]
    Exit {
        program: PathBuf,
        status: ExitStatus,
        text: String,
    },
}

/// Run to completion, capturing stdout and stderr. A non-zero exit is not an
/// error here; what one means is the caller's business.
pub(crate) fn run(invocation: &Invocation) -> Result<Output, CmdError> {
    let program = invocation.program().to_path_buf();

    let output = Command::new(&program)
        .args(invocation.arguments())
        .output()
        .context(SpawnSnafu {
            program: program.clone(),
        })?;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    Ok(Output {
        program,
        status: output.status,
        text,
    })
}

/// Indents whatever the program printed under the failure, or renders nothing
/// when it printed nothing at all.
struct Text<'a>(&'a str);

impl std::fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in self.0.trim_end().lines() {
            write!(f, "\n  {line}")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::util::cmd::Invocation;

    /// A program that behaves however the test needs it to.
    fn program(script: &str) -> Invocation {
        Invocation::new("sh").args(["-c", script])
    }

    #[test]
    fn captures_stdout_and_stderr_together() {
        let output = run(&program("echo out; echo err >&2")).expect("ran");

        assert!(output.status().success());
        assert!(output.text().contains("out"), "{:?}", output.text());
        assert!(output.text().contains("err"), "{:?}", output.text());
    }

    /// A non-zero exit is data, not an error -- the caller decides.
    #[test]
    fn a_non_zero_exit_is_not_an_error_by_itself() {
        assert_eq!(
            run(&program("exit 3")).expect("ran").status().code(),
            Some(3)
        );
    }

    /// Until the caller says it is.
    #[test]
    fn success_turns_a_non_zero_exit_into_an_error() {
        let error = run(&program("exit 3")).expect("ran").success().unwrap_err();

        let message = error.to_string();
        assert!(message.contains("sh"), "{message}");
        assert!(message.contains("exit status: 3"), "{message}");
    }

    /// The program's own diagnostics are the useful part of a failure, so they
    /// travel with it rather than being discarded.
    #[test]
    fn a_failure_carries_what_the_program_printed() {
        let error = run(&program("echo 'setup mode is disabled' >&2; exit 1"))
            .expect("ran")
            .success()
            .unwrap_err();

        assert!(
            error.to_string().contains("setup mode is disabled"),
            "{error}"
        );
    }

    #[test]
    fn success_is_quiet_when_the_program_succeeds() {
        run(&program("echo fine"))
            .expect("ran")
            .success()
            .expect("ok");
    }

    #[test]
    fn a_program_that_is_not_there_fails_to_spawn() {
        let error = run(&Invocation::new("/nonexistent/program")).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("could not run"), "{message}");
        assert!(message.contains("/nonexistent/program"), "{message}");
    }

    #[test]
    fn arguments_reach_the_program() {
        let output = run(&Invocation::new("sh").args([
            "-c",
            r#"echo "$@""#,
            "--",
            "--microsoft",
            "--firmware-builtin",
        ]))
        .expect("ran");

        assert_eq!(output.text().trim(), "--microsoft --firmware-builtin");
    }
}
