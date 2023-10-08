use anyhow::{bail, Result};
use indexmap::IndexMap;
use std::fmt::Write;

#[derive(Debug)]
pub enum MsbtToken {
    PlainText(String),
    NewLine,
    Arg(u16),
    TalkType {
        talk_type: u16,
        unknown: Option<String>,
    },
    Window {
        window_type: u16,
        speaker: String,
        variation: Option<String>,
    },
    Window2 {
        window_type: u16,
    },
    Wait {
        wait_type: u16,
        duration: Option<u32>,
    },
    Animation {
        animation_type: u16,
        target: String,
        animation: String,
    },
    Alias {
        actual: String,
        displayed: String,
    },
    PlayerName,
    MascotName,
    Fade {
        fade_type: u16,
        duration: u32,
        unknown: Option<u16>,
    },
    Icon(String),
    Localize {
        localize_type: u16,
        option1: String,
        option2: String,
    },
    Localize2 {
        localize_type: u16,
    },
    PictureShow {
        unknown: u32,
        picture: String,
        function: String,
    },
    PictureHide {
        unknown: u32,
        function: String,
    },
}

struct MsbtScanner<'a> {
    slice: &'a [u16],
    pos: usize,
}

impl<'a> MsbtScanner<'a> {
    pub fn new(msbt: &'a [u16]) -> Self {
        MsbtScanner {
            slice: msbt,
            pos: 0,
        }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.slice.len()
    }

    pub fn at_command_boundary(&self) -> Result<bool> {
        let c = self.peek()?;
        Ok(c == 0xA || c == 0xE || c == 0xF || c == 0x0)
    }

    pub fn next(&mut self) -> Result<u16> {
        if self.pos >= self.slice.len() {
            bail!("hit end of stream while parsing");
        }
        let v = self.slice[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn next_u32(&mut self) -> Result<u32> {
        let h1 = self.next()? as u32;
        let h2 = self.next()? as u32;
        Ok((h2 << 16) | h1)
    }

    pub fn peek(&self) -> Result<u16> {
        if self.pos >= self.slice.len() {
            bail!("hit end of stream while parsing");
        }
        Ok(self.slice[self.pos])
    }

    pub fn next_string(&mut self) -> Result<String> {
        let start = self.pos;
        while !self.at_end() && !self.at_command_boundary()? {
            self.pos += 1;
        }
        Ok(String::from_utf16(&self.slice[start..self.pos])?)
    }

    pub fn next_string_param(&mut self) -> Result<String> {
        // This is brittle but it's roughly how the game implements it.
        let char_count = self.next()? as usize >> 1;
        let end = self.pos + char_count;
        if end > self.slice.len() {
            bail!("string param length ran out of bounds");
        }
        let text = String::from_utf16(&self.slice[self.pos..end])?;
        self.pos += char_count;
        Ok(text)
    }
}

pub fn parse_msbt_script(contents: &IndexMap<String, Vec<u16>>) -> Result<String> {
    let mut out = String::new();
    for (k, v) in contents {
        pretty_print(&mut out, k, &parse_msbt_tokens(v)?)?;
    }
    Ok(out)
}

pub fn parse_msbt_entry(contents: &[u16]) -> Result<String> {
    let mut out = String::new();
    pretty_print_tokens(&mut out, &parse_msbt_tokens(contents)?)?;
    Ok(out)
}

fn parse_msbt_tokens(contents: &[u16]) -> Result<Vec<MsbtToken>> {
    let mut tokens = vec![];
    let mut scanner = MsbtScanner::new(contents);
    while !scanner.at_end() {
        let next = scanner.peek()?;
        tokens.push(match next {
            0xE => {
                scanner.next()?;
                let command = scanner.next()? as u32;
                match command {
                    1 => {
                        let arg = scanner.next()?;
                        let _ = scanner.next(); // Command length (swallowed)
                        MsbtToken::Arg(arg)
                    }
                    2 => {
                        let talk_type = scanner.next()?;
                        let _ = scanner.next(); // Command length (swallowed)
                        MsbtToken::TalkType {
                            talk_type,
                            unknown: if talk_type == 0 {
                                Some(scanner.next_string_param()?)
                            } else {
                                None
                            },
                        }
                    }
                    3 => {
                        let window_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        if window_type < 8 {
                            MsbtToken::Window {
                                window_type,
                                speaker: scanner.next_string_param()?,
                                variation: if window_type == 0 || window_type == 3 {
                                    Some(scanner.next_string_param()?)
                                } else {
                                    None
                                },
                            }
                        } else {
                            MsbtToken::Window2 { window_type }
                        }
                    }
                    4 => {
                        let wait_type = scanner.next()?;
                        let _ = scanner.next(); // Command length (swallowed)
                        MsbtToken::Wait {
                            wait_type,
                            duration: if wait_type == 3 {
                                Some(scanner.next_u32()?)
                            } else {
                                None
                            },
                        }
                    }
                    5 => {
                        let animation_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        MsbtToken::Animation {
                            animation_type,
                            target: scanner.next_string_param()?,
                            animation: scanner.next_string_param()?,
                        }
                    }
                    6 => {
                        let name_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        match name_type {
                            0 => MsbtToken::Alias {
                                actual: scanner.next_string_param()?,
                                displayed: scanner.next_string_param()?,
                            },
                            3 => MsbtToken::PlayerName,
                            5 => MsbtToken::MascotName,
                            _ => bail!("unknown name type {}", name_type),
                        }
                    }
                    7 => {
                        let fade_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        if fade_type > 1 {
                            bail!("expected fade type 0 or 1, found {}", fade_type);
                        }
                        MsbtToken::Fade {
                            fade_type,
                            duration: scanner.next_u32()?,
                            unknown: if fade_type == 1 {
                                Some(scanner.next()?)
                            } else {
                                None
                            },
                        }
                    }
                    8 => {
                        let icon_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        if icon_type != 2 {
                            bail!("expected icon type to be 2");
                        }
                        MsbtToken::Icon(scanner.next_string_param()?)
                    }
                    10 => {
                        let localize_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        if localize_type == 2 || localize_type == 3 {
                            MsbtToken::Localize2 { localize_type }
                        } else {
                            MsbtToken::Localize {
                                localize_type,
                                option1: scanner.next_string_param()?,
                                option2: scanner.next_string_param()?,
                            }
                        }
                    }
                    11 => {
                        let picture_type = scanner.next()?;
                        let _ = scanner.next()?; // Command length (swallowed)
                        if picture_type > 1 {
                            bail!("unsupported picture type '{}'", picture_type);
                        }
                        if picture_type == 0 {
                            MsbtToken::PictureShow {
                                unknown: scanner.next_u32()?,
                                picture: scanner.next_string_param()?,
                                function: scanner.next_string_param()?,
                            }
                        } else {
                            MsbtToken::PictureHide {
                                unknown: scanner.next_u32()?,
                                function: scanner.next_string_param()?,
                            }
                        }
                    }
                    _ => bail!("unknown command '{}'", command),
                }
            }
            0x0 => break,
            0xF => bail!("unexpected 0xF character in MSBT"),
            0xA => {
                scanner.next()?;
                MsbtToken::NewLine
            }
            _ => MsbtToken::PlainText(scanner.next_string()?),
        });
    }
    Ok(tokens)
}

pub fn pretty_print_tokenized_msbt_entry(tokens: &[MsbtToken]) -> Result<String> {
    let mut out = String::new();
    pretty_print_tokens(&mut out, tokens)?;
    Ok(out)
}

fn pretty_print(out: &mut String, key: &str, tokens: &[MsbtToken]) -> Result<()> {
    writeln!(out, "[{}]", key)?;
    pretty_print_tokens(out, tokens)?;
    out.push_str("\n\n");
    Ok(())
}

fn pretty_print_tokens(out: &mut String, tokens: &[MsbtToken]) -> Result<()> {
    for token in tokens {
        match token {
            MsbtToken::PlainText(text) => out.push_str(text),
            MsbtToken::NewLine => out.push('\n'),
            MsbtToken::Arg(arg) => write!(out, "$Arg({})", arg)?,
            MsbtToken::TalkType { talk_type, unknown } => match unknown {
                Some(v) => write!(out, "$Type({}, \"{}\")", talk_type, v)?,
                None => write!(out, "$Type({})", talk_type)?,
            },
            MsbtToken::Window {
                window_type,
                speaker,
                variation,
            } => match variation {
                Some(v) => write!(out, "$Window({}, \"{}\", \"{}\")", window_type, speaker, v)?,
                None => write!(out, "$Window({}, \"{}\")", window_type, speaker)?,
            },
            MsbtToken::Window2 { window_type } => write!(out, "$Window2({})", window_type)?,
            MsbtToken::Wait {
                wait_type,
                duration,
            } => match duration {
                Some(v) => write!(out, "$Wait({}, {})", wait_type, v)?,
                None => write!(out, "$Wait({})", wait_type)?,
            },
            MsbtToken::Animation {
                animation_type,
                target,
                animation,
            } => write!(
                out,
                "$Anim({}, \"{}\", \"{}\")",
                animation_type, target, animation
            )?,
            MsbtToken::Alias { actual, displayed } => {
                write!(out, "$Alias(\"{}\", \"{}\")", actual, displayed)?
            }
            MsbtToken::PlayerName => out.push_str("$P"),
            MsbtToken::MascotName => out.push_str("$M"),
            MsbtToken::Fade {
                fade_type,
                duration,
                unknown,
            } => match unknown {
                Some(unknown) => write!(out, "$Fade({}, {}, {})", fade_type, duration, unknown)?,
                None => write!(out, "$Fade({}, {})", fade_type, duration)?,
            },
            MsbtToken::Icon(name) => write!(out, "$Icon(\"{}\")", name)?,
            MsbtToken::Localize {
                localize_type,
                option1,
                option2,
            } => {
                if *localize_type == 0 {
                    write!(out, "$G(\"{}\", \"{}\")", option1, option2)?
                } else {
                    write!(
                        out,
                        "$G(\"{}\", \"{}\", {})",
                        option1, option2, localize_type
                    )?
                }
            }
            MsbtToken::Localize2 { localize_type } => write!(out, "$G2({})", localize_type)?,
            MsbtToken::PictureShow {
                unknown,
                picture,
                function,
            } => write!(out, "$Show({}, \"{}\", \"{}\")", unknown, picture, function)?,
            MsbtToken::PictureHide { unknown, function } => {
                write!(out, "$Hide({}, \"{}\")", unknown, function)?
            }
        }
    }
    Ok(())
}

enum PackedCommandArg<'a> {
    U16(u16),
    U32(u32),
    Str(Option<&'a str>),
}

struct CommandPacker<'a> {
    id: u16,
    sub_id: u16,
    args: Vec<PackedCommandArg<'a>>,
}

impl<'a> CommandPacker<'a> {
    pub fn new(id: u16, sub_id: u16) -> Self {
        CommandPacker {
            id,
            sub_id,
            args: vec![],
        }
    }

    pub fn int32(mut self, value: u32) -> Self {
        self.args.push(PackedCommandArg::U32(value));
        self
    }

    pub fn optional_int16(mut self, value: Option<u16>) -> Self {
        if let Some(value) = value {
            self.args.push(PackedCommandArg::U16(value));
        }
        self
    }

    pub fn string(mut self, value: Option<&'a str>) -> Self {
        self.args.push(PackedCommandArg::Str(value));
        self
    }

    pub fn pack(self, out: &mut Vec<u16>) {
        out.push(0xE);
        out.push(self.id);
        out.push(self.sub_id);
        out.push(0);
        let length_index = out.len() - 1;
        for arg in self.args {
            match arg {
                PackedCommandArg::U16(num) => out.push(num),
                PackedCommandArg::U32(num) => {
                    out.push((num & 0xFFFF) as u16);
                    out.push(((num & 0xFFFF0000) >> 16) as u16);
                }
                PackedCommandArg::Str(text) => {
                    if let Some(text) = text {
                        out.push(0);
                        let index = out.len() - 1;
                        out.extend(text.encode_utf16());
                        out[index] = ((out.len() - index - 1) * 2) as u16;
                    }
                }
            }
        }
        out[length_index] = ((out.len() - length_index - 1) * 2) as u16;
    }
}

pub fn pack_msbt_entries(entries: &IndexMap<String, Vec<MsbtToken>>) -> IndexMap<String, Vec<u16>> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), pack_msbt_entry(v)))
        .collect()
}

pub fn pack_msbt_entry(tokens: &[MsbtToken]) -> Vec<u16> {
    let mut packed = vec![];
    for token in tokens {
        match token {
            MsbtToken::PlainText(text) => packed.extend(text.encode_utf16()),
            MsbtToken::NewLine => packed.push(0xA),
            MsbtToken::Arg(arg) => {
                packed.push(0xE);
                packed.push(0x1);
                packed.push(*arg);
                packed.push(0);
            }
            MsbtToken::TalkType { talk_type, unknown } => CommandPacker::new(0x2, *talk_type)
                .string(unknown.as_deref())
                .pack(&mut packed),
            MsbtToken::Window {
                window_type,
                speaker,
                variation,
            } => CommandPacker::new(0x3, *window_type)
                .string(Some(speaker))
                .string(variation.as_deref())
                .pack(&mut packed),
            MsbtToken::Window2 { window_type } => {
                CommandPacker::new(0x3, *window_type).pack(&mut packed)
            }
            MsbtToken::Wait {
                wait_type,
                duration,
            } => {
                packed.push(0xE);
                packed.push(0x4);
                packed.push(*wait_type);
                packed.push(if duration.is_some() { 4 } else { 0 });
                if let Some(duration) = duration {
                    packed.push((duration & 0xFFFF) as u16);
                    packed.push(((duration & 0xFFFF0000) >> 16) as u16);
                }
            }
            MsbtToken::Animation {
                animation_type,
                target,
                animation,
            } => CommandPacker::new(0x5, *animation_type)
                .string(Some(target))
                .string(Some(animation))
                .pack(&mut packed),
            MsbtToken::Alias { actual, displayed } => CommandPacker::new(0x6, 0x0)
                .string(Some(actual))
                .string(Some(displayed))
                .pack(&mut packed),
            MsbtToken::PlayerName => {
                packed.push(0xE);
                packed.push(0x6);
                packed.push(0x3);
                packed.push(0x0);
            }
            MsbtToken::MascotName => {
                packed.push(0xE);
                packed.push(0x6);
                packed.push(0x5);
                packed.push(0x0);
            }
            MsbtToken::Fade {
                fade_type,
                duration,
                unknown,
            } => CommandPacker::new(0x7, *fade_type)
                .int32(*duration)
                .optional_int16(*unknown)
                .pack(&mut packed),
            MsbtToken::Icon(icon) => CommandPacker::new(0x8, 0x2)
                .string(Some(icon))
                .pack(&mut packed),
            MsbtToken::Localize {
                localize_type,
                option1,
                option2,
            } => CommandPacker::new(0xA, *localize_type)
                .string(Some(option1))
                .string(Some(option2))
                .pack(&mut packed),
            MsbtToken::Localize2 { localize_type } => {
                CommandPacker::new(0xA, *localize_type).pack(&mut packed)
            }
            MsbtToken::PictureShow {
                unknown,
                picture,
                function,
            } => CommandPacker::new(0xB, 0x0)
                .int32(*unknown)
                .string(Some(picture))
                .string(Some(function))
                .pack(&mut packed),
            MsbtToken::PictureHide { unknown, function } => CommandPacker::new(0xB, 0x1)
                .int32(*unknown)
                .string(Some(function))
                .pack(&mut packed),
        }
    }
    packed.push(0);
    packed
}
