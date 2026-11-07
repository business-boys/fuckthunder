//! Token-to-`Response` dispatch and per-keyword parsers.

use fuckthunder_core::{
    CapabilitySet, Error, FlagSet, MailboxName, ResponseCode, Result, StatusData, StatusItem,
    TaggedStatus,
};

use crate::fetch::parse_fetch_items;
use crate::token::{tokenize, Token};

use super::{ListEntry, NamespaceEntry, NamespaceResponse, Response, Untagged};

/// Parses one complete response buffer.
///
/// The buffer must contain exactly one response, including any literal octets
/// and the final CRLF.
///
/// # Errors
///
/// Returns [`Error::Parse`] for malformed input.
pub fn parse_response(input: &[u8]) -> Result<Response> {
    let tokens = tokenize(input)?;
    let Some(first) = tokens.first() else {
        return Err(Error::Parse("empty response".into()));
    };
    match first {
        Token::Atom(b"+") => {
            let (code, text) = split_code(&tokens[1..])?;
            Ok(Response::Continuation { code, text })
        }
        Token::Atom(b"*") => Ok(Response::Untagged(parse_untagged(&tokens[1..])?)),
        Token::Atom(tag) => {
            let tag = String::from_utf8_lossy(tag).into_owned();
            let Some(status_token) = tokens.get(1) else {
                return Err(Error::Parse("tagged response without status".into()));
            };
            let status_text = status_token
                .to_text()
                .ok_or_else(|| Error::Parse("tagged status is not a string".into()))?;
            let status = TaggedStatus::parse(&status_text)
                .ok_or_else(|| Error::Parse(format!("unknown tagged status {status_text}")))?;
            let (code, text) = split_code(&tokens[2..])?;
            Ok(Response::Tagged {
                tag,
                status,
                code,
                text,
            })
        }
        other => Err(Error::Parse(format!(
            "response does not start with a tag, '*', or '+': {:?}",
            other.to_text()
        ))),
    }
}

fn split_code(tokens: &[Token<'_>]) -> Result<(Option<ResponseCode>, String)> {
    let mut code = None;
    let mut rest = tokens;
    if let Some(Token::Code(inner)) = tokens.first() {
        let words = flatten(inner);
        code = Some(ResponseCode::parse(&words)?);
        rest = &tokens[1..];
    }
    let text = rest
        .iter()
        .filter_map(Token::to_text)
        .collect::<Vec<_>>()
        .join(" ");
    Ok((code, text))
}

fn flatten(tokens: &[Token<'_>]) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokens {
        match token {
            Token::List(items) | Token::Code(items) => out.extend(flatten(items)),
            other => {
                if let Some(text) = other.to_text() {
                    out.push(text);
                }
            }
        }
    }
    out
}

fn parse_untagged(tokens: &[Token<'_>]) -> Result<Untagged> {
    let Some(first) = tokens.first() else {
        return Err(Error::Parse("untagged response without keyword".into()));
    };
    // Numeric-prefixed untagged responses: `* 12 EXISTS`, `* 3 FETCH (...)`.
    if let Some(number) = first.as_u32() {
        let Some(keyword_token) = tokens.get(1) else {
            return Err(Error::Parse(
                "numeric untagged response without keyword".into(),
            ));
        };
        let keyword = keyword_token
            .to_text()
            .ok_or_else(|| Error::Parse("untagged keyword is not a string".into()))?;
        let args = &tokens[2..];
        return match keyword.to_ascii_uppercase().as_str() {
            "EXISTS" => Ok(Untagged::Exists(number)),
            "RECENT" => Ok(Untagged::Recent(number)),
            "EXPUNGE" => Ok(Untagged::Expunge(number)),
            "FETCH" => parse_fetch(number, args),
            _ => Ok(Untagged::Other {
                atom: keyword,
                text: args
                    .iter()
                    .filter_map(Token::to_text)
                    .collect::<Vec<_>>()
                    .join(" "),
            }),
        };
    }
    let keyword = first
        .to_text()
        .ok_or_else(|| Error::Parse("untagged keyword is not a string".into()))?;
    let upper = keyword.to_ascii_uppercase();
    let args = &tokens[1..];

    match upper.as_str() {
        "OK" | "NO" | "BAD" | "PREAUTH" | "BYE" => {
            let (code, text) = split_code(args)?;
            Ok(match upper.as_str() {
                "OK" => Untagged::Ok { code, text },
                "NO" => Untagged::No { code, text },
                "BAD" => Untagged::Bad { code, text },
                "PREAUTH" => Untagged::PreAuth { code, text },
                _ => Untagged::Bye { code, text },
            })
        }
        "CAPABILITY" => {
            let atoms: Vec<String> = args.iter().filter_map(Token::to_text).collect();
            Ok(Untagged::Capability(CapabilitySet::from_atoms(atoms)))
        }
        "LIST" => Ok(Untagged::List(parse_list_entry(args)?)),
        "LSUB" => Ok(Untagged::Lsub(parse_list_entry(args)?)),
        "STATUS" => parse_status(args),
        "SEARCH" | "SORT" => {
            let numbers = args
                .iter()
                .map(|token| {
                    token
                        .as_u32()
                        .ok_or_else(|| Error::Parse("non-numeric SEARCH result".into()))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Untagged::Search(numbers))
        }
        "FLAGS" => {
            let atoms: Vec<String> = args
                .first()
                .and_then(Token::as_list)
                .unwrap_or_default()
                .iter()
                .filter_map(Token::to_text)
                .collect();
            Ok(Untagged::Flags(FlagSet::parse(atoms)))
        }
        "NAMESPACE" => parse_namespace(args),
        "ENABLED" => Ok(Untagged::Enabled(
            args.iter().filter_map(Token::to_text).collect(),
        )),
        "ID" => parse_id(args),
        "QUOTA" => parse_quota(args),
        _ => Ok(Untagged::Other {
            atom: keyword,
            text: args
                .iter()
                .filter_map(Token::to_text)
                .collect::<Vec<_>>()
                .join(" "),
        }),
    }
}

fn parse_list_entry(args: &[Token<'_>]) -> Result<ListEntry> {
    if args.len() < 3 {
        return Err(Error::Parse(
            "LIST needs attributes, delimiter and name".into(),
        ));
    }
    let attributes = args[0]
        .as_list()
        .ok_or_else(|| Error::Parse("LIST attributes are not a list".into()))?
        .iter()
        .filter_map(Token::to_text)
        .collect();
    let delimiter = match &args[1] {
        token if token.is_nil() => None,
        token => {
            let text = token
                .to_text()
                .ok_or_else(|| Error::Parse("LIST delimiter is not a string".into()))?;
            text.chars().next()
        }
    };
    let name_text = args[2]
        .to_text()
        .ok_or_else(|| Error::Parse("LIST name is not a string".into()))?;
    let name = MailboxName::decode_wire(&name_text)?;
    Ok(ListEntry {
        attributes,
        delimiter,
        name,
    })
}

fn parse_status(args: &[Token<'_>]) -> Result<Untagged> {
    if args.len() < 2 {
        return Err(Error::Parse("STATUS needs a mailbox and items".into()));
    }
    let mailbox_text = args[0]
        .to_text()
        .ok_or_else(|| Error::Parse("STATUS mailbox is not a string".into()))?;
    let mailbox = MailboxName::decode_wire(&mailbox_text)?;
    let items = args[1]
        .as_list()
        .ok_or_else(|| Error::Parse("STATUS items are not a list".into()))?;
    let mut data = StatusData::default();
    for pair in items.chunks_exact(2) {
        let name = pair[0]
            .to_text()
            .ok_or_else(|| Error::Parse("STATUS item name missing".into()))?;
        let item = StatusItem::parse(&name)
            .ok_or_else(|| Error::Parse(format!("unknown STATUS item {name}")))?;
        let value = pair[1]
            .as_u64()
            .ok_or_else(|| Error::Parse(format!("STATUS {name} is not numeric")))?;
        data.set(item, value)?;
    }
    Ok(Untagged::Status { mailbox, data })
}

fn parse_fetch(seq: u32, args: &[Token<'_>]) -> Result<Untagged> {
    let items = args
        .first()
        .and_then(Token::as_list)
        .ok_or_else(|| Error::Parse("FETCH without item list".into()))?;
    Ok(Untagged::Fetch(Box::new(parse_fetch_items(seq, items)?)))
}

fn parse_namespace(args: &[Token<'_>]) -> Result<Untagged> {
    if args.len() < 3 {
        return Err(Error::Parse("NAMESPACE needs three fields".into()));
    }
    Ok(Untagged::Namespace(NamespaceResponse {
        personal: parse_namespace_list(&args[0])?,
        other_users: parse_namespace_list(&args[1])?,
        shared: parse_namespace_list(&args[2])?,
    }))
}

fn parse_namespace_list(token: &Token<'_>) -> Result<Vec<NamespaceEntry>> {
    if token.is_nil() {
        return Ok(Vec::new());
    }
    let list = token
        .as_list()
        .ok_or_else(|| Error::Parse("namespace field is not a list".into()))?;
    list.iter()
        .map(|entry| {
            let pair = entry
                .as_list()
                .ok_or_else(|| Error::Parse("namespace entry is not a list".into()))?;
            if pair.len() < 2 {
                return Err(Error::Parse(
                    "namespace entry needs prefix and delimiter".into(),
                ));
            }
            let prefix_text = pair[0]
                .to_text()
                .ok_or_else(|| Error::Parse("namespace prefix missing".into()))?;
            let delimiter = if pair[1].is_nil() {
                None
            } else {
                pair[1].to_text().and_then(|s| s.chars().next())
            };
            Ok(NamespaceEntry {
                prefix: MailboxName::decode_wire(&prefix_text)?,
                delimiter,
            })
        })
        .collect()
}

fn parse_id(args: &[Token<'_>]) -> Result<Untagged> {
    let Some(token) = args.first() else {
        return Err(Error::Parse("ID without payload".into()));
    };
    if token.is_nil() {
        return Ok(Untagged::Id(Vec::new()));
    }
    let list = token
        .as_list()
        .ok_or_else(|| Error::Parse("ID payload is not a list".into()))?;
    let pairs = list
        .chunks_exact(2)
        .filter_map(|pair| {
            let key = pair[0].to_text()?;
            let value = if pair[1].is_nil() {
                None
            } else {
                pair[1].to_text()
            };
            Some((key, value))
        })
        .collect();
    Ok(Untagged::Id(pairs))
}

fn parse_quota(args: &[Token<'_>]) -> Result<Untagged> {
    if args.len() < 2 {
        return Err(Error::Parse(
            "QUOTA needs a root and a resource list".into(),
        ));
    }
    let root = args[0]
        .to_text()
        .ok_or_else(|| Error::Parse("QUOTA root missing".into()))?;
    let list = args[1]
        .as_list()
        .ok_or_else(|| Error::Parse("QUOTA resources are not a list".into()))?;
    if list.len() < 3 {
        return Err(Error::Parse("QUOTA resource triple is incomplete".into()));
    }
    Ok(Untagged::Quota {
        root,
        resource: list[0].to_text().unwrap_or_default(),
        usage: list[1]
            .as_u64()
            .ok_or_else(|| Error::Parse("QUOTA usage is not numeric".into()))?,
        limit: list[2]
            .as_u64()
            .ok_or_else(|| Error::Parse("QUOTA limit is not numeric".into()))?,
    })
}
