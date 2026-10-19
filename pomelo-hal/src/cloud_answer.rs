//! Reading the cloud answer document.
//!
//! One document and one reader, shared by every backend that can produce it. What the C side writes
//! over the bridge is this crate's own vocabulary already — the same fault words the management page
//! reads out of the same bytes — so the only thing left to do with it is to turn it back into
//! [`CloudAnswer`], and that is what is here rather than in `pomelo-hal-esp32`.
//!
//! # Why not a JSON dependency
//!
//! Because the document is four known shapes written by one file in this tree, and the crate's
//! dependencies are chosen rather than accumulated. What is below reads exactly the syntax
//! `board_cloud.c` emits — objects one level deep, strings, `\u` escapes for control characters,
//! and UTF-8 passed through byte for byte — and nothing about a general parser's job: no comments,
//! no exponents, no nesting to descend. [`parse`] is a pure function over a `&str` and is tested as
//! one, which is what a parser in a device image should be.
//!
//! # What a document that makes no sense reads as
//!
//! [`CloudFault::Failed`] with no code and no message: the fault that says the request failed and
//! nothing it cannot back up. This is not a defensive branch — a truncated answer is a real
//! possibility here, since the answer buffer is a fixed size on a board with a few hundred
//! kilobytes of heap — and a page that drew a half-read document as a list of zero instances would
//! be telling a person their account is empty.

use crate::types::{
    CloudAction, CloudAnswer, CloudFault, CloudInstance, CloudInstanceState, CloudProvider,
};

/// Reads the answer document `board_cloud.c` writes.
///
/// Total, and never fails: see the module documentation for what a document it cannot read comes
/// out as.
pub fn parse(document: &str) -> CloudAnswer {
    // A failure first, because it is the one shape that can arrive with nothing else in it — and
    // because a `{"error":…}` document is what the C side writes for every reason a request did not
    // produce a list or an acceptance.
    if let Some(value) = field(document, "error") {
        return CloudAnswer::Refused {
            fault: CloudFault::from_code(&text_of(value)),
            // Two empty strings for the faults that have no provider sentence behind them, which
            // is every fault but `cloud_api`: what the page shows for those is its own words for
            // the fault, and a page that had to invent a code as well would be inventing twice.
            code: field(document, "code").map(text_of).unwrap_or_default(),
            message: field(document, "message").map(text_of).unwrap_or_default(),
        };
    }

    if let Some(value) = field(document, "instances") {
        let provider = provider_of(document);

        return CloudAnswer::Instances {
            provider,
            region: field(document, "region").map(text_of).unwrap_or_default(),
            instances: instances_in(value, provider),
            failed: providers_in(field(document, "failed").unwrap_or_default()),
        };
    }

    if let Some(value) = field(document, "action") {
        if let Some(action) = CloudAction::from_name(&text_of(value)) {
            return CloudAnswer::Accepted {
                provider: provider_of(document),
                action,
                request_id: field(document, "request_id")
                    .map(text_of)
                    .unwrap_or_default(),
            };
        }
    }

    CloudAnswer::Refused {
        fault: CloudFault::Failed,
        code: String::new(),
        message: String::new(),
    }
}

/// The provider an answer names, or the first one when it names none.
///
/// A document with instances in it and no `provider` field is not one this side writes, and the
/// default is the first product rather than a `None` the enum has no room for: what a page does
/// with a list is draw it, and every row carries what is needed to act on it.
fn provider_of(document: &str) -> CloudProvider {
    field(document, "provider")
        .and_then(|value| CloudProvider::from_name(&text_of(value)))
        .unwrap_or(CloudProvider::Cvm)
}

/// The instances of a list, read out of the text between `instances` and the end of the document.
///
/// One object at a time, by the braces: the document is flat and its objects have no braces inside
/// them, so the first `{` after where the last one ended is the next instance — with no recursion
/// and no state to get wrong.
///
/// An object with no `id` is skipped rather than carried: an instance the page cannot aim a command
/// at is not a row worth drawing. A document truncated inside an object keeps whatever whole
/// objects came before it, which is the answer to "did the buffer fill up" that a person can use.
///
/// `kind` on a row is which API listed it. A merged list has one on every row — that is how a list
/// of two APIs is a list of machines and not a list of lines — and a row that has none (a document
/// from a board that only knew one product) is the document's own provider.
fn instances_in(list: &str, provider: CloudProvider) -> Vec<CloudInstance> {
    let mut instances = Vec::new();
    let mut rest = list;

    // `all` is a document's word for "these came from both", and no row can be a power command's
    // target: a row without a `kind` in a merged list is a row from a board that could not say, and
    // the first product is the honest guess rather than a row nothing can be sent for.
    let fallback = if provider.is_product() {
        provider
    } else {
        CloudProvider::Cvm
    };

    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let object = &after[..end];
        rest = &after[end + 1..];

        let Some(id) = field(object, "id").map(text_of) else {
            continue;
        };

        instances.push(CloudInstance {
            id,
            name: field(object, "name").map(text_of).unwrap_or_default(),
            state: field(object, "state")
                .map(|value| CloudInstanceState::from_name(&text_of(value)))
                .unwrap_or_default(),
            ip: field(object, "ip").map(text_of).unwrap_or_default(),
            provider: field(object, "kind")
                .and_then(|value| CloudProvider::from_name(&text_of(value)))
                .filter(|kind| kind.is_product())
                .unwrap_or(fallback),
            // The data centre the row was listed from. Empty only in a document from a board that
            // did not say, where the power command falls back to the key's own region.
            region: field(object, "region").map(text_of).unwrap_or_default(),
        });
    }

    instances
}

/// The products named in a `"failed":["lighthouse"]` array.
///
/// Empty for a single product's list, which either arrived whole or answered with something else
/// entirely: `failed` is the word a merged list uses to say which half is missing.
fn providers_in(list: &str) -> Vec<CloudProvider> {
    list.split('"')
        .skip(1)
        .step_by(2)
        .filter_map(CloudProvider::from_name)
        .filter(|provider| provider.is_product())
        .collect()
}

/// The value of `"key"`, with the colon and the space after it skipped.
///
/// The first occurrence. Not ambiguous for a document written by one file that writes each key once
/// per object, and the one case that would be — `"id"` inside `"request_id"` — cannot happen: this
/// searches for the quotes as well, and `request_id` has an underscore where that quote would be.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let rest = &text[text.find(&needle)? + needle.len()..];
    rest.trim_start().strip_prefix(':').map(str::trim_start)
}

/// A value as the text it is: the string's own characters, or a bare token as it stands.
fn text_of(value: &str) -> String {
    if value.starts_with('"') {
        return string_of(value).unwrap_or_default();
    }

    value
        .find(|ch| matches!(ch, ',' | '}' | ']'))
        .map_or(value, |end| &value[..end])
        .trim()
        .to_string()
}

/// The string a `"…"` value is, with its escapes decoded.
///
/// `None` for a value that does not start with a quote. A string with no closing quote — a document
/// cut short by the answer buffer filling up — hands back what there was of it rather than nothing,
/// since half of a name still tells a person which instance it is.
fn string_of(value: &str) -> Option<String> {
    let rest = value.strip_prefix('"')?;
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars();

    while let Some(ch) = chars.next() {
        if ch == '"' {
            break;
        }

        if ch != '\\' {
            out.push(ch);
            continue;
        }

        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                // Four hex digits of a code point, which is how the C side writes the control
                // characters it will not put in a document raw. Anything else, including a
                // surrogate half, is dropped rather than guessed at.
                let digits: String = chars.by_ref().take(4).collect();
                if let Ok(code) = u32::from_str_radix(&digits, 16) {
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                    }
                }
            }
            Some(other) => out.push(other),
            None => break,
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_reads_back_as_instances() {
        let document = concat!(
            r#"{"ok":1,"provider":"lighthouse","region":"ap-guangzhou","instances":["#,
            r#"{"id":"lhins-aaa","name":"blog","state":"RUNNING","ip":"1.2.3.4","#,
            r#""kind":"lighthouse","region":"ap-tokyo"},"#,
            r#"{"id":"lhins-bbb","name":"","state":"STOPPED","ip":""}"#,
            r#"],"count":2}"#,
        );

        let CloudAnswer::Instances {
            provider,
            region,
            instances,
            ..
        } = parse(document)
        else {
            panic!("a list should read as instances");
        };

        assert_eq!(provider, CloudProvider::Lighthouse);
        assert_eq!(region, "ap-guangzhou");
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].id, "lhins-aaa");
        assert_eq!(instances[0].name, "blog");
        assert_eq!(instances[0].state, CloudInstanceState::Running);
        assert_eq!(instances[0].ip, "1.2.3.4");
        // The data centre a row was listed from, which is the one a power command has to name: a key
        // saved in Guangzhou must not stop a machine in Tokyo.
        assert_eq!(instances[0].region, "ap-tokyo");
        // A document without one is a board that did not say, and the command falls back to the key.
        assert!(instances[1].region.is_empty());
        // A stopped instance with no address at all is still a row: it is what the panel draws a
        // "power on" button for.
        assert_eq!(instances[1].state, CloudInstanceState::Stopped);
        assert!(instances[1].name.is_empty());
    }

    #[test]
    fn an_acceptance_carries_its_receipt() {
        let document =
            r#"{"ok":1,"provider":"cvm","action":"stop","request_id":"4f1e-9c"}"#;

        let CloudAnswer::Accepted {
            provider,
            action,
            request_id,
        } = parse(document)
        else {
            panic!("an acceptance should read as one");
        };

        assert_eq!(provider, CloudProvider::Cvm);
        assert_eq!(action, CloudAction::Stop);
        assert_eq!(request_id, "4f1e-9c");
    }

    #[test]
    fn a_refusal_the_provider_wrote_keeps_its_sentence() {
        let document = concat!(
            r#"{"error":"cloud_api","code":"AuthFailure.SignatureFailure","#,
            r#""message":"签名验证失败"}"#,
        );

        let CloudAnswer::Refused {
            fault,
            code,
            message,
        } = parse(document)
        else {
            panic!("a refusal should read as one");
        };

        assert_eq!(fault, CloudFault::Api);
        assert_eq!(code, "AuthFailure.SignatureFailure");
        // UTF-8 passes through the document as itself, and comes back as itself.
        assert_eq!(message, "签名验证失败");
    }

    #[test]
    fn a_fault_with_no_sentence_behind_it_reads_as_its_code() {
        let CloudAnswer::Refused { fault, code, .. } = parse(r#"{"error":"cloud_clock"}"#) else {
            panic!("a fault should read as a refusal");
        };

        assert_eq!(fault, CloudFault::ClockNotSet);
        assert!(code.is_empty());
    }

    #[test]
    fn a_document_that_makes_no_sense_is_the_fault_that_says_nothing() {
        for document in ["", "not json at all", r#"{"ok":0}"#] {
            let CloudAnswer::Refused { fault, .. } = parse(document) else {
                panic!("{document:?} should read as a refusal");
            };
            assert_eq!(fault, CloudFault::Failed, "{document:?}");
        }
    }

    #[test]
    fn an_account_with_no_instances_is_an_empty_list_and_not_a_failure() {
        // The one document that would be a failure if it were read by shape rather than by intent:
        // nothing in it is wrong, and what a person needs to see is an empty page and not "the
        // request failed".
        let CloudAnswer::Instances { instances, .. } =
            parse(r#"{"ok":1,"provider":"cvm","instances":[],"count":0}"#)
        else {
            panic!("an empty list should read as a list");
        };

        assert!(instances.is_empty());
    }

    #[test]
    fn a_document_cut_short_keeps_the_objects_that_arrived() {
        let document = concat!(
            r#"{"provider":"cvm","instances":[{"id":"ins-1","state":"RUNNING"},"#,
            r#"{"id":"ins-2","state":"STAR"#,
        );

        let CloudAnswer::Instances { instances, .. } = parse(document) else {
            panic!("a truncated list should still read as a list");
        };

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].id, "ins-1");
    }

    #[test]
    fn an_object_with_no_id_is_not_a_row() {
        let document = r#"{"instances":[{"name":"nameless"},{"id":"ins-9"}]}"#;

        let CloudAnswer::Instances { instances, .. } = parse(document) else {
            panic!("a list should read as instances");
        };

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].id, "ins-9");
    }

    #[test]
    fn a_merged_list_carries_the_product_on_every_row() {
        // The document a panel asks for now: one call, both APIs, one list. `kind` is what a power
        // button is addressed by, so a row without one would be a row whose button went nowhere.
        let document = concat!(
            r#"{"ok":1,"provider":"all","region":"","instances":["#,
            r#"{"id":"lhins-aaa","name":"blog","state":"RUNNING","ip":"1.2.3.4","kind":"lighthouse"},"#,
            r#"{"id":"ins-bbb","name":"web","state":"STOPPED","ip":"5.6.7.8","kind":"cvm"}"#,
            r#"],"count":2}"#,
        );

        let CloudAnswer::Instances {
            provider,
            region,
            instances,
            failed,
        } = parse(document)
        else {
            panic!("a merged list should read as instances");
        };

        assert_eq!(provider, CloudProvider::All);
        assert!(region.is_empty());
        assert!(failed.is_empty());
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].provider, CloudProvider::Lighthouse);
        assert_eq!(instances[1].provider, CloudProvider::Cvm);
    }

    #[test]
    fn a_merged_list_names_the_half_that_did_not_come_back() {
        let document = concat!(
            r#"{"ok":1,"provider":"all","region":"","instances":["#,
            r#"{"id":"lhins-aaa","state":"RUNNING","kind":"lighthouse"}"#,
            r#"],"count":1,"failed":["cvm"]}"#,
        );

        let CloudAnswer::Instances {
            instances,
            failed,
            ..
        } = parse(document)
        else {
            panic!("a merged list should read as instances");
        };

        assert_eq!(instances.len(), 1);
        assert_eq!(failed, [CloudProvider::Cvm]);
    }

    #[test]
    fn escapes_come_back_as_the_characters_they_stand_for() {
        let document = r#"{"instances":[{"id":"a\"b\\c\nd\u0009e"}]}"#;

        let CloudAnswer::Instances { instances, .. } = parse(document) else {
            panic!("a list should read as instances");
        };

        assert_eq!(instances[0].id, "a\"b\\c\nd\te");
    }
}
