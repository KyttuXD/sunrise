//! Native access to the server console: operator-visible logs and command source output.
use super::{Game, commands::execute::Source};
use mod_api::{CommandFeedback, CommandResult, TextFormError};
use std::sync::mpsc::{Sender, channel};
use sunrise_client::{logging, text::Text};

#[derive(Clone, Debug)]
pub(in crate::game) enum Output {
    Discard,
    Plain(Sender<String>),
    Stream(Sender<Result<CommandFeedback, TextFormError>>),
}

impl Game<'_> {
    /// `console_line` for each line logged since the last server tick, while
    /// an enabled mod listens; the lines its listeners log are not kept.
    pub(super) fn native_console_lines(&mut self) {
        let listened = mod_api::__private::enabled_listened(mod_api::HookId::ConsoleLine);
        self.dispatch_console_lines(logging::take_tapped(listened));
    }

    pub(super) fn dispatch_console_lines(&mut self, lines: Vec<logging::TappedLine>) {
        for line in lines {
            let level = line.level.as_str();
            logging::untapped(|| {
                self.native_console_line(level, &line.thread, &line.message, line.dropped);
            });
        }
    }

    /// `Commands::run_command`: run as the console with no source output.
    pub(super) fn native_run_command(&mut self, line: &str) -> bool {
        self.run_console_command(line, Output::Discard)
    }

    /// `Commands::run_command_with_output`: take a snapshot of the plain output on return.
    pub(super) fn native_run_command_with_output(&mut self, line: &str) -> CommandResult {
        let (send, receive) = channel();
        let success = self.run_console_command(line, Output::Plain(send));
        CommandResult::new(success, receive.try_iter().collect())
    }

    /// Run at owner permission from the world spawn. Nested runs get their own source, leaving
    /// the caller's execution state, result callbacks and output untouched.
    pub(in crate::game) fn run_console_command(&mut self, line: &str, output: Output) -> bool {
        if self.lan.is_none() {
            return false;
        }
        let silent = matches!(output, Output::Discard);
        let line = if silent {
            line
        } else {
            let line = line.trim();
            let line = line.strip_prefix('/').unwrap_or(line);
            if line.trim().is_empty() {
                return false;
            }
            line
        };
        let source = self.server_source();
        let dim = source.dimension;
        let source = Source {
            native_console: Some(output),
            silent,
            level: Some(dim),
            ..source
        };
        let filter = std::mem::replace(&mut self.native_command_filter, true);
        let outer_exec = std::mem::take(&mut self.exec);
        let outer_results = self.cmds.results.take();
        let outer_chat = self.chat_capture.take();
        let outer_admins = std::mem::take(&mut self.admin_feedback);
        let success = self.with_source(source, |g| {
            if g.dim != dim {
                g.command_failure(Text::translate(
                    "argument.dimension.invalid",
                    &[Text::literal(dim.id())],
                ));
                return false;
            }
            g.as_console(|g| g.run_command(line))
        });
        self.send_admin_feedback(None);
        self.admin_feedback = outer_admins;
        self.chat_capture = outer_chat;
        self.exec = outer_exec;
        self.cmds.results = outer_results;
        self.native_command_filter = filter;
        success
    }

    pub(super) fn native_console_source(&self) -> bool {
        self.command_source
            .as_ref()
            .is_some_and(|source| source.native_console.is_some())
    }

    /// Deliver to the command source rather than the ambient player chat capture or log.
    pub(super) fn send_command_output(
        &mut self,
        kind: fn(mod_api::Text) -> CommandFeedback,
        text: Text,
    ) {
        match self
            .command_source
            .as_ref()
            .and_then(|s| s.native_console.as_ref())
        {
            Some(Output::Plain(output)) => {
                let _ = output.send(text.plain());
            }
            Some(Output::Stream(output)) => {
                // Only a receiver that has stopped listening can refuse this send.
                let _ = output.send(sunrise_client_lib::api_text::to_api(&text).map(kind));
            }
            Some(Output::Discard) => {}
            None => self.system_message(text),
        }
    }
}
