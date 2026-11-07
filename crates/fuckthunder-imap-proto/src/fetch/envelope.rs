//! `ENVELOPE` and address-list decoding.

use fuckthunder_core::{Address, Envelope, Error, Result};

use crate::token::Token;

use super::optional_text;

/// Parses an `ENVELOPE` list.
///
/// # Errors
///
/// Fails when the list is too short or an address is malformed.
pub fn parse_envelope(token: &Token<'_>) -> Result<Envelope> {
    let list = token
        .as_list()
        .ok_or_else(|| Error::Parse("ENVELOPE is not a list".into()))?;
    if list.len() < 10 {
        return Err(Error::Parse(format!(
            "ENVELOPE needs 10 fields, got {}",
            list.len()
        )));
    }
    Ok(Envelope {
        date: optional_text(&list[0]),
        subject: optional_text(&list[1]),
        from: parse_addresses(&list[2])?,
        sender: parse_addresses(&list[3])?,
        reply_to: parse_addresses(&list[4])?,
        to: parse_addresses(&list[5])?,
        cc: parse_addresses(&list[6])?,
        bcc: parse_addresses(&list[7])?,
        in_reply_to: optional_text(&list[8]),
        message_id: optional_text(&list[9]),
    })
}

fn parse_addresses(token: &Token<'_>) -> Result<Vec<Address>> {
    if token.is_nil() {
        return Ok(Vec::new());
    }
    let list = token
        .as_list()
        .ok_or_else(|| Error::Parse("address list is not a list".into()))?;
    list.iter().map(parse_address).collect()
}

fn parse_address(token: &Token<'_>) -> Result<Address> {
    let fields = token
        .as_list()
        .ok_or_else(|| Error::Parse("address is not a list".into()))?;
    if fields.len() < 4 {
        return Err(Error::Parse("address needs 4 fields".into()));
    }
    Ok(Address {
        name: optional_text(&fields[0]),
        adl: optional_text(&fields[1]),
        mailbox: optional_text(&fields[2]),
        host: optional_text(&fields[3]),
    })
}
