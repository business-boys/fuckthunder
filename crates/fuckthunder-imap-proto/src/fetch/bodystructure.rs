//! `BODYSTRUCTURE` decoding for single-part and multipart bodies.

use fuckthunder_core::{Error, Result};

use crate::token::Token;

use super::{as_u64, optional_text, parse_envelope, BodyStructure};

/// Parses a `BODYSTRUCTURE` list.
///
/// # Errors
///
/// Fails when required fields are missing or mistyped.
pub fn parse_body_structure(token: &Token<'_>) -> Result<BodyStructure> {
    let fields = token
        .as_list()
        .ok_or_else(|| Error::Parse("BODYSTRUCTURE is not a list".into()))?;
    if fields.is_empty() {
        return Err(Error::Parse("empty BODYSTRUCTURE".into()));
    }
    if matches!(fields[0], Token::List(_)) {
        return parse_multipart(fields);
    }
    parse_single_part(fields)
}

fn parse_multipart(fields: &[Token<'_>]) -> Result<BodyStructure> {
    let mut children = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        if matches!(fields[index], Token::List(_)) {
            children.push(parse_body_structure(&fields[index])?);
            index += 1;
        } else {
            break;
        }
    }
    let subtype = fields
        .get(index)
        .and_then(Token::to_text)
        .unwrap_or_else(|| "MIXED".into());
    Ok(BodyStructure {
        media_type: "MULTIPART".into(),
        media_subtype: subtype.to_ascii_uppercase(),
        children,
        ..BodyStructure::default()
    })
}

fn parse_single_part(fields: &[Token<'_>]) -> Result<BodyStructure> {
    if fields.len() < 7 {
        return Err(Error::Parse("single-part BODYSTRUCTURE too short".into()));
    }
    let media_type = fields[0].to_text().unwrap_or_default().to_ascii_uppercase();
    let media_subtype = fields[1].to_text().unwrap_or_default().to_ascii_uppercase();
    let mut structure = BodyStructure {
        media_type,
        media_subtype,
        params: parse_params(&fields[2]),
        id: optional_text(&fields[3]),
        description: optional_text(&fields[4]),
        encoding: fields[5].to_text().unwrap_or_default(),
        size: as_u64(&fields[6])?,
        ..BodyStructure::default()
    };
    let mut index = 7;
    if structure.is_message() {
        if let Some(envelope) = fields.get(index) {
            structure.envelope = Some(parse_envelope(envelope)?);
        }
        index += 1;
        if let Some(body) = fields.get(index) {
            structure.nested = Some(Box::new(parse_body_structure(body)?));
        }
        index += 1;
        if let Some(lines) = fields.get(index) {
            structure.lines = lines.as_u64();
        }
        index += 1;
    } else if structure.media_type == "TEXT" {
        if let Some(lines) = fields.get(index) {
            structure.lines = lines.as_u64();
        }
        index += 1;
    }
    if let Some(md5) = fields.get(index).and_then(optional_text) {
        structure.md5 = Some(md5);
    }
    index += 1;
    if let Some(disposition) = fields.get(index) {
        structure.disposition = parse_disposition(disposition);
    }
    Ok(structure)
}

fn parse_params(token: &Token<'_>) -> Vec<(String, String)> {
    let Some(list) = token.as_list() else {
        return Vec::new();
    };
    list.chunks_exact(2)
        .filter_map(|pair| Some((pair[0].to_text()?, pair[1].to_text()?)))
        .collect()
}

fn parse_disposition(token: &Token<'_>) -> Option<(String, Vec<(String, String)>)> {
    let list = token.as_list()?;
    let kind = list.first()?.to_text()?;
    let params = list.get(1).map_or_else(Vec::new, parse_params);
    Some((kind, params))
}
