//! Display shell history with statement breaks without interpreting or executing it.

use std::iter::Peekable;
use std::str::Chars;

pub(crate) fn history_layout(command: &str) -> String {
    // A heredoc body can contain arbitrary text; leave its layout intact.
    if command.contains("<<") {
        return command.to_owned();
    }

    let mut formatter = Formatter {
        characters: command.chars().peekable(),
        contexts: Vec::new(),
        formatted: String::with_capacity(command.len()),
        pending_space: false,
    };
    while let Some(character) = formatter.characters.next() {
        formatter.push(character);
    }

    if formatter.contexts.is_empty() {
        formatter.formatted
    } else {
        command.to_owned()
    }
}

struct Formatter<'a> {
    characters: Peekable<Chars<'a>>,
    contexts: Vec<char>,
    formatted: String,
    pending_space: bool,
}

impl Formatter<'_> {
    fn push(&mut self, character: char) {
        if self.copy_literal(character) || self.collapse_whitespace(character) {
            return;
        }

        let starts_comment = self.contexts.is_empty()
            && character == '#'
            && (self.pending_space || self.formatted.is_empty() || self.formatted.ends_with('\n'));
        if self.pending_space && !self.formatted.is_empty() && !self.formatted.ends_with('\n') {
            self.formatted.push(' ');
        }
        self.pending_space = false;
        self.formatted.push(character);

        // Escapes and comments must stay intact before punctuation is interpreted.
        match character {
            '\\' => self.copy_next(),
            '#' if starts_comment => {
                for character in self.characters.by_ref() {
                    self.formatted.push(character);
                    if character == '\n' {
                        break;
                    }
                }
            }
            _ => self.punctuation(character),
        }
    }

    fn copy_literal(&mut self, character: char) -> bool {
        let context = self.contexts.last().copied();
        if !matches!(context, Some('\'' | '`')) {
            return false;
        }

        self.formatted.push(character);
        if character == '\\' && context == Some('`') {
            self.copy_next();
        } else if Some(character) == context {
            self.contexts.pop();
        }

        true
    }

    fn collapse_whitespace(&mut self, character: char) -> bool {
        if !self.contexts.is_empty() || !matches!(character, ' ' | '\t' | '\r' | '\n') {
            return false;
        }

        if character == '\n' {
            if !self.formatted.ends_with('\n') {
                self.formatted.push('\n');
            }
            self.pending_space = false;
        } else {
            self.pending_space = true;
        }

        true
    }

    fn punctuation(&mut self, character: char) {
        let context = self.contexts.last().copied();

        // Nested substitutions keep inner quotes from closing an outer quote.
        match character {
            '$' if matches!(self.characters.peek(), Some('(' | '{')) => {
                let opening = self
                    .characters
                    .next()
                    .expect("peeked substitution delimiter");
                self.formatted.push(opening);
                self.contexts.push(if opening == '(' { ')' } else { '}' });
            }
            '"' if context == Some('"') => {
                self.contexts.pop();
            }
            '"' => self.contexts.push('"'),
            '\'' if context != Some('"') => self.contexts.push('\''),
            '`' => self.contexts.push('`'),
            '(' if context == Some(')') => self.contexts.push(')'),
            ')' | '}' if Some(character) == context => {
                self.contexts.pop();
            }
            _ if context.is_none() => self.statement_break(character),
            _ => {}
        }
    }

    fn statement_break(&mut self, character: char) {
        match character {
            ';' => {
                while self.characters.peek() == Some(&';') {
                    self.copy_next();
                }
                if self.characters.peek() == Some(&'&') {
                    self.copy_next();
                }
            }
            '&' | '|' if self.characters.peek() == Some(&character) => self.copy_next(),
            _ => return,
        }

        self.formatted.push('\n');
    }

    fn copy_next(&mut self) {
        if let Some(character) = self.characters.next() {
            self.formatted.push(character);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::history_layout;

    #[test]
    fn history_layout_preserves_shell_literals_and_breaks_only_top_level_statements() {
        for (input, expected) in [
            ("RUN  echo  hi; echo done", "RUN echo hi;\necho done"),
            (
                r#"echo "a;  b" && printf '%s' "猫""#,
                "echo \"a;  b\" &&\nprintf '%s' \"猫\"",
            ),
            (
                r#"echo "$(printf 'a; b')" || echo ${value}"#,
                "echo \"$(printf 'a; b')\" ||\necho ${value}",
            ),
            (r"echo a\;b; echo done", "echo a\\;b;\necho done"),
            (
                "echo hi # keep ; && comment\necho done",
                "echo hi # keep ; && comment\necho done",
            ),
            (
                "echo `printf 'a; b'` && echo done",
                "echo `printf 'a; b'` &&\necho done",
            ),
            (
                "case x in x) echo hi;; y) echo y;;& esac",
                "case x in x) echo hi;;\ny) echo y;;&\nesac",
            ),
        ] {
            assert_eq!(history_layout(input), expected, "{input}");
        }
    }

    #[test]
    fn heredocs_and_unmatched_delimiters_keep_the_original_history() {
        for input in [
            "cat <<EOF\n arbitrary ; && text\nEOF\n echo done",
            "echo  'unfinished; echo done",
            "echo  \"$(unfinished; echo done",
            "",
        ] {
            assert_eq!(history_layout(input), input);
        }
    }
}
