use std::error::Error;
use std::fmt::{Display, Write};
use std::ops::Range;

use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use indexmap::IndexMap;
use itertools::Itertools;
use logos::{Lexer, Logos};

use crate::MsbtToken;

type Result<T> = std::result::Result<T, ParseError>;
type Location = Range<usize>;

#[derive(Debug)]
pub enum ParseError {
    Aggregated(Vec<ParseError>),
    BadNumber(Location, String),
    EndOfFile,
    UnexpectedToken(Location, String, String),
    LexerError(Location, String),
    DuplicateKey(Location, String),
}

impl ParseError {
    pub fn report(&self, source_script: &str) {
        let mut writer = StandardStream::stderr(ColorChoice::Always);
        let config = term::Config::default();
        let mut files = SimpleFiles::new();
        let id = files.add("source", source_script);
        match self {
            ParseError::Aggregated(errors) => {
                for error in errors {
                    let _ = term::emit(&mut writer, &config, &files, &error.to_diagnostic(id));
                }
            }
            _ => {
                let _ = term::emit(&mut writer, &config, &files, &self.to_diagnostic(id));
            }
        };
    }

    pub fn to_diagnostic(&self, file_id: usize) -> Diagnostic<usize> {
        match self {
            ParseError::Aggregated(_) => unimplemented!(),
            ParseError::BadNumber(loc, msg) => Diagnostic::error()
                .with_message("bad number")
                .with_labels(vec![
                    Label::primary(file_id, loc.to_owned()).with_message(msg)
                ]),
            ParseError::EndOfFile => Diagnostic::error().with_message("unexpected end of file"),
            ParseError::UnexpectedToken(loc, wanted, actual) => Diagnostic::error()
                .with_message("unexpected token")
                .with_labels(vec![Label::primary(file_id, loc.to_owned())
                    .with_message(format!("found '{}', expected '{}'", actual, wanted))]),
            ParseError::LexerError(loc, token) => Diagnostic::error()
                .with_message("unexpected token")
                .with_labels(vec![Label::primary(file_id, loc.to_owned())
                    .with_message(format!("unexpected token '{}'", token))]),
            ParseError::DuplicateKey(loc, key) => Diagnostic::error()
                .with_message("duplicate key")
                .with_labels(vec![Label::primary(file_id, loc.to_owned())
                    .with_message(format!("duplicate key '{}'", key))]),
        }
    }
}

impl Error for ParseError {}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Aggregated(errors) => {
                write!(f, "{}", errors.iter().map(|e| e.to_string()).join("; "))
            }
            ParseError::BadNumber(loc, message) => write!(f, "{:?}: {}", loc, message),
            ParseError::EndOfFile => f.write_str("unexpected EOF"),
            ParseError::UnexpectedToken(loc, wanted, actual) => {
                write!(f, "{:?}: expected '{}', found '{}'", loc, wanted, actual)
            }
            ParseError::LexerError(loc, text) => {
                write!(f, "{:?}: unexpected token '{}'", loc, text)
            }
            ParseError::DuplicateKey(loc, key) => write!(f, "{:?}: duplicate key {}", loc, key),
        }
    }
}

impl From<ParseError> for Vec<ParseError> {
    fn from(value: ParseError) -> Self {
        match value {
            ParseError::Aggregated(errors) => errors,
            _ => vec![value],
        }
    }
}

#[derive(Debug, Copy, Clone, Logos, PartialEq, Eq)]
enum Token {
    #[token("(")]
    LeftParen,
    #[token(")")]
    RightParen,
    #[token(",")]
    Comma,
    #[token("$Arg")]
    Arg,
    #[token("$Type")]
    Type,
    #[token("$Window")]
    Window,
    #[token("$Window2")]
    Window2,
    #[token("$Wait")]
    Wait,
    #[token("$Anim")]
    Anim,
    #[token("$Alias")]
    Alias,
    #[token("$P")]
    PlayerName,
    #[token("$M")]
    MascotName,
    #[token("$Fade")]
    Fade,
    #[token("$Icon")]
    Icon,
    #[token("$G")]
    Localize,
    #[token("$G2")]
    Localize2,
    #[token("$Show")]
    Show,
    #[token("$Hide")]
    Hide,
    #[regex("(\\r\\n|\\r|\\n)")]
    NewLine,
    #[regex("-?\\d*")]
    Number,
    #[regex("\"[^\"\\r\\n$]*\"")]
    Str,
    #[regex("\\[[ \t]*[a-zA-Z0-9#+_'\"]+[ \t]*\\]")]
    Identifier,
    #[regex("[^(),$\\d\\r\\n][^$\"\\r\\n\\d]*")]
    Text,
    #[regex("\"[^\"\\r\\n$]+\\r\\n")]
    UnterminatedStrHack,
    Error,
}

impl Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::LeftParen => f.write_char('('),
            Token::RightParen => f.write_char(')'),
            Token::Comma => f.write_char(','),
            Token::Arg => f.write_str("$Arg"),
            Token::Type => f.write_str("$Type"),
            Token::Window => f.write_str("$Window"),
            Token::Window2 => f.write_str("$Window2"),
            Token::Wait => f.write_str("$Wait"),
            Token::Anim => f.write_str("$Anim"),
            Token::Alias => f.write_str("$Alias"),
            Token::PlayerName => f.write_str("$P"),
            Token::MascotName => f.write_str("$M"),
            Token::Fade => f.write_str("$Fade"),
            Token::Icon => f.write_str("$Icon"),
            Token::Localize => f.write_str("$G"),
            Token::Localize2 => f.write_str("$G2"),
            Token::Show => f.write_str("$Show"),
            Token::Hide => f.write_str("$Hide"),
            Token::NewLine => f.write_str("\\n"),
            Token::Number => f.write_str("number"),
            Token::Str => f.write_str("string"),
            Token::Identifier => f.write_str("identifier"),
            Token::Text => f.write_str("text"),
            Token::UnterminatedStrHack => f.write_str("text"),
            Token::Error => f.write_str("error"),
        }
    }
}

struct PeekableLexer<'source> {
    lexer: Lexer<'source, Token>,
    peeked: Option<Option<Token>>,
}

impl<'source> PeekableLexer<'source> {
    pub fn new(source: &'source str) -> Self {
        Self {
            lexer: Token::lexer(source),
            peeked: None,
        }
    }

    pub fn peek(&mut self) -> Option<Token> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lexer.next().map(|r| r.unwrap_or(Token::Error)));
        }
        self.peeked.unwrap()
    }

    pub fn slice(&self) -> &'source str {
        self.lexer.slice()
    }

    pub fn span(&self) -> Range<usize> {
        self.lexer.span()
    }
}

impl<'source> Iterator for PeekableLexer<'source> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if let Some(peeked) = self.peeked.take() {
            peeked
        } else {
            self.lexer.next().map(|r| r.unwrap_or(Token::Error))
        }
    }
}

struct Parser<'source> {
    lexer: PeekableLexer<'source>,
}

impl<'source> Parser<'source> {
    pub fn next_keyed_entry(&mut self) -> Result<(String, Vec<MsbtToken>)> {
        let key = self.expect_identifier()?;
        self.expect(Token::NewLine)?;
        Ok((key, self.next_entry()?))
    }

    pub fn next_entry(&mut self) -> Result<Vec<MsbtToken>> {
        let mut commands = vec![];
        loop {
            if self.at_end() || self.peek()? == Token::Identifier {
                break;
            }
            match self.next()? {
                Token::Arg => {
                    self.expect(Token::LeftParen)?;
                    let arg = self.expect_number()?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Arg(arg as u16));
                }
                Token::Type => {
                    self.expect(Token::LeftParen)?;
                    let talk_type = self.expect_number()? as u16;
                    self.skip_whitespace()?;
                    let unknown = self.next_optional(Parser::expect_string)?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::TalkType { talk_type, unknown });
                }
                Token::Window => {
                    self.expect(Token::LeftParen)?;
                    let window_type = self.expect_number()? as u16;
                    self.expect(Token::Comma)?;
                    let speaker = self.expect_string()?;
                    let variation = self.next_optional(Parser::expect_string)?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Window {
                        window_type,
                        speaker,
                        variation,
                    });
                }
                Token::Window2 => {
                    self.expect(Token::LeftParen)?;
                    let window_type = self.expect_number()? as u16;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Window2 { window_type });
                }
                Token::Wait => {
                    self.expect(Token::LeftParen)?;
                    let wait_type = self.expect_number()? as u16;
                    let duration = self.next_optional(Parser::expect_number)?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Wait {
                        wait_type,
                        duration,
                    });
                }
                Token::Anim => {
                    self.expect(Token::LeftParen)?;
                    let animation_type = self.expect_number()? as u16;
                    self.expect(Token::Comma)?;
                    let target = self.expect_string()?;
                    self.expect(Token::Comma)?;
                    let animation = self.expect_string()?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Animation {
                        animation_type,
                        target,
                        animation,
                    });
                }
                Token::PlayerName => commands.push(MsbtToken::PlayerName),
                Token::MascotName => commands.push(MsbtToken::MascotName),
                Token::Alias => {
                    self.expect(Token::LeftParen)?;
                    let actual = self.expect_string()?;
                    self.expect(Token::Comma)?;
                    let displayed = self.expect_string()?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Alias { actual, displayed });
                }
                Token::Fade => {
                    self.expect(Token::LeftParen)?;
                    let fade_type = self.expect_number()? as u16;
                    self.expect(Token::Comma)?;
                    let duration = self.expect_number()?;
                    let unknown = self.next_optional(Parser::expect_number)?.map(|v| v as u16);
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Fade {
                        fade_type,
                        duration,
                        unknown,
                    });
                }
                Token::Icon => {
                    self.expect(Token::LeftParen)?;
                    let icon = self.expect_string()?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Icon(icon));
                }
                Token::Localize => {
                    self.expect(Token::LeftParen)?;
                    let option1 = self.expect_string()?;
                    self.expect(Token::Comma)?;
                    let option2 = self.expect_string()?;
                    let localize_type = self
                        .next_optional(Parser::expect_number)?
                        .map(|v| v as u16)
                        .unwrap_or(0);
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Localize {
                        localize_type,
                        option1,
                        option2,
                    });
                }
                Token::Localize2 => {
                    self.expect(Token::LeftParen)?;
                    let localize_type = self.expect_number()? as u16;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::Localize2 { localize_type });
                }
                Token::Show => {
                    self.expect(Token::LeftParen)?;
                    let unknown = self.expect_number()?;
                    self.expect(Token::Comma)?;
                    let picture = self.expect_string()?;
                    self.expect(Token::Comma)?;
                    let function = self.expect_string()?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::PictureShow {
                        unknown,
                        picture,
                        function,
                    });
                }
                Token::Hide => {
                    self.expect(Token::LeftParen)?;
                    let unknown = self.expect_number()?;
                    self.expect(Token::Comma)?;
                    let function = self.expect_string()?;
                    self.expect(Token::RightParen)?;
                    commands.push(MsbtToken::PictureHide { unknown, function });
                }
                Token::NewLine => commands.push(MsbtToken::NewLine),
                Token::Text
                | Token::Str
                | Token::UnterminatedStrHack
                | Token::Number
                | Token::LeftParen
                | Token::RightParen
                | Token::Comma => Parser::push_or_extend_text(&mut commands, self.lexer.slice()),
                _ => {
                    return Err(ParseError::LexerError(
                        self.location(),
                        self.lexer.slice().to_string(),
                    ));
                }
            }
        }
        while let Some(MsbtToken::NewLine) = commands.last() {
            commands.pop();
        }
        Ok(commands)
    }

    fn push_or_extend_text(commands: &mut Vec<MsbtToken>, new_text: &str) {
        if let Some(MsbtToken::PlainText(text)) = commands.last_mut() {
            text.push_str(new_text);
        } else {
            commands.push(MsbtToken::PlainText(new_text.to_string()));
        }
    }

    pub fn skip_to_next_entry(&mut self) -> Result<()> {
        while !self.at_end() {
            let token = self.peek()?;
            if token == Token::Identifier {
                break;
            } else {
                self.next()?;
            }
        }
        Ok(())
    }

    pub fn skip_whitespace(&mut self) -> Result<()> {
        let token = self.peek()?;
        if token == Token::Text && self.lexer.slice().trim().is_empty() {
            self.next()?;
        }
        Ok(())
    }

    pub fn at_end(&mut self) -> bool {
        self.lexer.peek().is_none()
    }

    pub fn next_optional<T, F: FnOnce(&mut Self) -> Result<T>>(
        &mut self,
        func: F,
    ) -> Result<Option<T>> {
        if self.peek()? == Token::Comma {
            self.expect(Token::Comma)?;
            Ok(Some(func(self)?))
        } else {
            Ok(None)
        }
    }

    pub fn expect_number(&mut self) -> Result<u32> {
        self.expect(Token::Number)?;
        self.lexer
            .slice()
            .parse::<u32>()
            .map_err(|err| ParseError::BadNumber(self.location(), err.to_string()))
    }

    pub fn expect_string(&mut self) -> Result<String> {
        self.expect(Token::Str)?;
        Ok(self.lexer.slice()[1..self.lexer.slice().len() - 1].to_string())
    }

    pub fn expect_identifier(&mut self) -> Result<String> {
        self.expect(Token::Identifier)?;
        Ok(self.lexer.slice()[1..self.lexer.slice().len() - 1]
            .trim()
            .to_string())
    }

    pub fn expect(&mut self, expected: Token) -> Result<()> {
        self.skip_whitespace()?;
        let actual = self.next()?;
        if expected == actual {
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken(
                self.location(),
                expected.to_string(),
                self.lexer.slice().to_string(),
            ))
        }
    }

    pub fn next(&mut self) -> Result<Token> {
        let token = self.lexer.next().ok_or(ParseError::EndOfFile)?;
        if let Token::Error = token {
            Err(ParseError::LexerError(
                self.location(),
                self.lexer.slice().to_string(),
            ))
        } else {
            Ok(token)
        }
    }

    pub fn peek(&mut self) -> Result<Token> {
        let token = self.lexer.peek().ok_or(ParseError::EndOfFile)?;
        if let Token::Error = token {
            Err(ParseError::LexerError(
                self.location(),
                self.lexer.slice().to_string(),
            ))
        } else {
            Ok(token)
        }
    }

    fn location(&self) -> Location {
        self.lexer.span()
    }
}

pub fn parse_astra_script(source: &str) -> Result<IndexMap<String, Vec<MsbtToken>>> {
    let mut parser = Parser {
        lexer: PeekableLexer::new(source),
    };
    let mut entries = IndexMap::new();
    let mut errors = vec![];
    while !parser.at_end() {
        match parser.next_keyed_entry() {
            Ok((key, tokens)) => {
                entries.insert(key, tokens);
            }
            Err(err) => {
                parser.skip_to_next_entry()?;
                errors.push(err);
            }
        }
    }
    if errors.is_empty() {
        Ok(entries)
    } else if errors.len() == 1 {
        Err(errors.remove(0))
    } else {
        Err(ParseError::Aggregated(errors))
    }
}

pub fn parse_astra_script_entry(source: &str) -> Result<Vec<MsbtToken>> {
    Parser {
        lexer: PeekableLexer::new(source),
    }
    .next_entry()
}

pub fn pack_astra_script(source: &str) -> Result<IndexMap<String, Vec<u16>>> {
    let entries = parse_astra_script(source)?;
    Ok(crate::pack_msbt_entries(&entries))
}

/// Convert between script entries (organized in a map) and a single script.
/// Currently does NOT validate key or value, so you can break this if you're trying to.
pub fn convert_entries_to_astra_script(
    entries: &IndexMap<String, String>,
) -> anyhow::Result<String> {
    let mut output = String::new();
    for (k, v) in entries {
        writeln!(output, "[{}]", k)?;
        writeln!(output, "{}", v)?;
        writeln!(output)?;
    }
    Ok(output)
}

/// Convert an Astra script (one string containing all entries) to a key / value map.
pub fn convert_astra_script_to_entries(script: &str) -> anyhow::Result<IndexMap<String, String>> {
    let mut converted = IndexMap::new();
    for (k, v) in parse_astra_script(script)? {
        converted.insert(k, crate::pretty_print_tokenized_msbt_entry(&v)?);
    }
    Ok(converted)
}
