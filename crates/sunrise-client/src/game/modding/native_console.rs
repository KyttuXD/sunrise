//! Native access to the server console: operator-visible logs and command source output.
use super::{Game, commands::execute::Source};
use mod_api::{CommandFeedback, CommandResult, TextFormError};
use std::sync::mpsc::Sender;
use sunrise_client::{logging, text::Text};

pub(in crate::game) enum Output {
    Plain(Vec<String>),
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
        self.run_console_command(line, None).success
    }

    /// `Commands::run_command_with_output`: collect the console source's output as plain text.
    pub(super) fn native_run_command_with_output(&mut self, line: &str) -> CommandResult {
        self.run_console_command(line, Some(Output::Plain(Vec::new())))
    }

    /// Run at owner permission from the world spawn. Nested runs use their own source and output,
    /// leaving the calling command's execution state, result callbacks and output untouched.
    pub(in crate::game) fn run_console_command(
        &mut self,
        line: &str,
        output: Option<Output>,
    ) -> CommandResult {
        if self.lan.is_none() {
            return CommandResult::default();
        }
        let line = if output.is_some() {
            let line = line.trim();
            let line = line.strip_prefix('/').unwrap_or(line);
            if line.trim().is_empty() {
                return CommandResult::default();
            }
            line
        } else {
            line
        };
        let source = Source {
            native_console: true,
            silent: output.is_none(),
            ..self.server_source()
        };
        let outer = self.command_source.replace(source);
        let filter = std::mem::replace(&mut self.native_command_filter, true);
        let outer_exec = std::mem::take(&mut self.exec);
        let outer_results = self.cmds.results.take();
        let outer_output = std::mem::replace(&mut self.native_console_output, output);
        let outer_chat = self.chat_capture.take();
        let success = self.as_console(|g| g.run_command(line));
        self.chat_capture = outer_chat;
        self.exec = outer_exec;
        self.cmds.results = outer_results;
        self.native_command_filter = filter;
        self.command_source = outer;
        let output = std::mem::replace(&mut self.native_console_output, outer_output);
        let feedback = match output {
            Some(Output::Plain(lines)) => lines,
            _ => Vec::new(),
        };
        CommandResult::new(success, feedback)
    }

    pub(super) fn native_console_source(&self) -> bool {
        self.command_source
            .as_ref()
            .is_some_and(|source| source.native_console)
    }

    /// Deliver to the command source rather than the ambient player chat capture or log.
    pub(super) fn send_command_output(
        &mut self,
        kind: fn(mod_api::Text) -> CommandFeedback,
        text: Text,
    ) {
        if !self.native_console_source() {
            self.system_message(text);
            return;
        }
        match &mut self.native_console_output {
            Some(Output::Plain(lines)) => lines.push(text.plain()),
            Some(Output::Stream(output)) => {
                // Only a receiver that has stopped listening can refuse this send.
                let _ = output.send(sunrise_client_lib::api_text::to_api(&text).map(kind));
            }
            None => {}
        }
    }
}
