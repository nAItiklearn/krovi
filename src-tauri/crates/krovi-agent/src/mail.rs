//! The safe email flow. Krovi NEVER sends mail and never reads mail: this module only checks the
//! shape of the addresses and builds a *compose* link. The user reviews the draft on a
//! confirmation card, it opens in their own mail app (Gmail web, Outlook web, or the default
//! `mailto:` client), and they press Send there themselves.

use std::fmt;

use crate::intent::MailProvider;
use crate::url::{classify_host, encode_component, encode_keeping, HostKind, MAX_MAILTO_LEN};

pub const MAX_RECIPIENTS: usize = 5;
pub const MAX_SUBJECT_CHARS: usize = 200;
pub const MAX_BODY_CHARS: usize = 1500;
/// Longest compose link we build (`mailto:` links share the tighter [`MAX_MAILTO_LEN`]).
pub const MAX_COMPOSE_URL: usize = MAX_MAILTO_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailError {
    BadAddress(String),
    TooManyRecipients,
    SubjectTooLong,
    BodyTooLong,
    LinkTooLong,
}

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailError::BadAddress(a) => write!(
                f,
                "\"{a}\" is not an email address Krovi can use. Say the full address, for example name@example.com."
            ),
            MailError::TooManyRecipients => {
                write!(f, "Krovi prepares drafts for up to {MAX_RECIPIENTS} recipients.")
            }
            MailError::SubjectTooLong => {
                write!(f, "The subject is too long (over {MAX_SUBJECT_CHARS} characters).")
            }
            MailError::BodyTooLong => {
                write!(f, "The message is too long for a draft link (over {MAX_BODY_CHARS} characters).")
            }
            MailError::LinkTooLong => write!(f, "That draft is too long to open as a link."),
        }
    }
}

impl std::error::Error for MailError {}

/// What to put in the compose window. `to` may be empty: the user picks the recipient themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailDraft {
    pub provider: MailProvider,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// Check an address's shape and normalise it (domain lower-cased). Deliberately narrower than
/// RFC 5321: a plain local part (`A-Z a-z 0-9 . _ + ' -`) and a real host name with a dot.
pub fn validate_address(raw: &str) -> Result<String, MailError> {
    let s = raw.trim();
    let bad = || MailError::BadAddress(s.to_string());
    if s.is_empty() || s.len() > 254 || s.matches('@').count() != 1 {
        return Err(bad());
    }
    let (local, domain) = s.split_once('@').ok_or_else(bad)?;
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'\'' | b'-'));
    let domain = domain.to_ascii_lowercase();
    if !local_ok || classify_host(&domain) != Some(HostKind::Name) {
        return Err(bad());
    }
    Ok(format!("{local}@{domain}"))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_param(url: &mut String, first: &mut bool, name: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    url.push(if *first { '?' } else { '&' });
    *first = false;
    url.push_str(name);
    url.push('=');
    url.push_str(value);
}

/// Validate a draft and build its compose link.
///
/// * Gmail: `https://mail.google.com/mail/?view=cm&fs=1&to=..&su=..&body=..`
/// * Outlook: `https://outlook.live.com/mail/0/deeplink/compose?to=..&subject=..&body=..`
/// * Default: `mailto:<to>?subject=..&body=..`
///
/// Subject and body are collapsed to one line and percent-encoded, so nothing the user said can
/// add a parameter or a recipient of its own.
pub fn compose_url(d: &EmailDraft) -> Result<String, MailError> {
    if d.to.len() > MAX_RECIPIENTS {
        return Err(MailError::TooManyRecipients);
    }
    let to =
        d.to.iter()
            .map(|a| validate_address(a))
            .collect::<Result<Vec<_>, _>>()?;
    let subject = one_line(&d.subject);
    let body = one_line(&d.body);
    if subject.chars().count() > MAX_SUBJECT_CHARS {
        return Err(MailError::SubjectTooLong);
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(MailError::BodyTooLong);
    }
    let to_joined = to.join(",");
    let (subject_e, body_e) = (encode_component(&subject), encode_component(&body));

    let url = match d.provider {
        MailProvider::Gmail => {
            let mut u = String::from("https://mail.google.com/mail/?view=cm&fs=1");
            let mut first = false; // the query already started
            push_param(&mut u, &mut first, "to", &encode_component(&to_joined));
            push_param(&mut u, &mut first, "su", &subject_e);
            push_param(&mut u, &mut first, "body", &body_e);
            u
        }
        MailProvider::Outlook => {
            let mut u = String::from("https://outlook.live.com/mail/0/deeplink/compose");
            let mut first = true;
            push_param(&mut u, &mut first, "to", &encode_component(&to_joined));
            push_param(&mut u, &mut first, "subject", &subject_e);
            push_param(&mut u, &mut first, "body", &body_e);
            u
        }
        MailProvider::SystemDefault => {
            let mut u = format!("mailto:{}", encode_keeping(&to_joined, b"@,"));
            let mut first = true;
            push_param(&mut u, &mut first, "subject", &subject_e);
            push_param(&mut u, &mut first, "body", &body_e);
            u
        }
    };
    if url.len() > MAX_COMPOSE_URL {
        return Err(MailError::LinkTooLong);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::url::{check_openable, Openable};

    fn draft(p: MailProvider, to: &[&str], subject: &str, body: &str) -> EmailDraft {
        EmailDraft {
            provider: p,
            to: to.iter().map(|s| s.to_string()).collect(),
            subject: subject.into(),
            body: body.into(),
        }
    }

    #[test]
    fn valid_addresses_are_normalised() {
        for (raw, want) in [
            ("sam@x.com", "sam@x.com"),
            ("Sam.Smith+tag@Example.CO.UK", "Sam.Smith+tag@example.co.uk"),
            ("  o'brien@mail.example.org ", "o'brien@mail.example.org"),
            ("a_b-c@sub.domain.io", "a_b-c@sub.domain.io"),
        ] {
            assert_eq!(validate_address(raw).as_deref(), Ok(want), "{raw}");
        }
    }

    #[test]
    fn bad_addresses() {
        for bad in [
            "",
            "sam",
            "sam@",
            "@x.com",
            "sam@x",
            "sam@x.c",
            "sam@@x.com",
            "a@b@x.com",
            ".sam@x.com",
            "sam.@x.com",
            "sa..m@x.com",
            "sam smith@x.com",
            "sam@x .com",
            "sam@-x.com",
            "sam@localhost",
            "sam@127.0.0.1",
            "sam@[::1]",
            "<sam@x.com>",
            "sam@x.com,evil@y.com",
            "sam@x.com?bcc=evil@y.com",
            "sam%40x.com",
            "s\u{e5}m@x.com",
            "sam@x.com\r\nBcc: evil@y.com",
        ] {
            assert!(validate_address(bad).is_err(), "{bad:?}");
        }
        let long_local = format!("{}@x.com", "a".repeat(65));
        assert!(validate_address(&long_local).is_err());
        assert!(validate_address(&format!("{}@x.com", "a".repeat(64))).is_ok());
    }

    #[test]
    fn gmail_link() {
        let d = draft(
            MailProvider::Gmail,
            &["sam@x.com"],
            "The trip",
            "See you at noon",
        );
        assert_eq!(
            compose_url(&d).unwrap(),
            "https://mail.google.com/mail/?view=cm&fs=1&to=sam%40x.com&su=The%20trip&body=See%20you%20at%20noon"
        );
    }

    #[test]
    fn outlook_link() {
        let d = draft(MailProvider::Outlook, &["sam@x.com"], "The trip", "Hello");
        assert_eq!(
            compose_url(&d).unwrap(),
            "https://outlook.live.com/mail/0/deeplink/compose?to=sam%40x.com&subject=The%20trip&body=Hello"
        );
    }

    #[test]
    fn mailto_link() {
        let d = draft(
            MailProvider::SystemDefault,
            &["sam@x.com"],
            "The trip",
            "Hello there",
        );
        assert_eq!(
            compose_url(&d).unwrap(),
            "mailto:sam@x.com?subject=The%20trip&body=Hello%20there"
        );
        let d = draft(MailProvider::SystemDefault, &["a@x.com", "b@y.org"], "", "");
        assert_eq!(compose_url(&d).unwrap(), "mailto:a@x.com,b@y.org");
    }

    #[test]
    fn empty_fields_are_left_out() {
        let g = compose_url(&draft(MailProvider::Gmail, &[], "", "")).unwrap();
        assert_eq!(g, "https://mail.google.com/mail/?view=cm&fs=1");
        let o = compose_url(&draft(MailProvider::Outlook, &[], "", "")).unwrap();
        assert_eq!(o, "https://outlook.live.com/mail/0/deeplink/compose");
        let m = compose_url(&draft(MailProvider::SystemDefault, &[], "Hi", "")).unwrap();
        assert_eq!(m, "mailto:?subject=Hi");
        let o = compose_url(&draft(MailProvider::Outlook, &["a@x.com"], "", "Hi")).unwrap();
        assert_eq!(
            o,
            "https://outlook.live.com/mail/0/deeplink/compose?to=a%40x.com&body=Hi"
        );
    }

    #[test]
    fn text_cannot_inject_parameters_or_recipients() {
        for p in [
            MailProvider::Gmail,
            MailProvider::Outlook,
            MailProvider::SystemDefault,
        ] {
            let d = draft(
                p,
                &["sam@x.com"],
                "Hi&to=evil@y.com",
                "x&bcc=evil@y.com\r\nBcc: z@y.com",
            );
            let url = compose_url(&d).unwrap();
            assert!(!url.contains("evil@"), "{url}");
            assert!(!url.contains('\r') && !url.contains('\n'));
            assert!(!url.contains("&bcc="), "{url}");
            assert_eq!(
                url.matches("to=").count(),
                usize::from(p != MailProvider::SystemDefault)
            );
        }
    }

    #[test]
    fn unicode_is_percent_encoded() {
        let d = draft(
            MailProvider::Gmail,
            &["sam@x.com"],
            "Caf\u{e9} \u{1f600}",
            "",
        );
        assert!(compose_url(&d)
            .unwrap()
            .ends_with("su=Caf%C3%A9%20%F0%9F%98%80"));
    }

    #[test]
    fn limits() {
        assert_eq!(
            compose_url(&draft(MailProvider::Gmail, &["notanaddress"], "", "")),
            Err(MailError::BadAddress("notanaddress".into()))
        );
        let many: Vec<String> = (0..=MAX_RECIPIENTS)
            .map(|i| format!("u{i}@x.com"))
            .collect();
        let d = EmailDraft {
            provider: MailProvider::Gmail,
            to: many,
            subject: String::new(),
            body: String::new(),
        };
        assert_eq!(compose_url(&d), Err(MailError::TooManyRecipients));
        let d = draft(
            MailProvider::Gmail,
            &[],
            &"s".repeat(MAX_SUBJECT_CHARS + 1),
            "",
        );
        assert_eq!(compose_url(&d), Err(MailError::SubjectTooLong));
        let d = draft(
            MailProvider::Gmail,
            &[],
            "",
            &"b".repeat(MAX_BODY_CHARS + 1),
        );
        assert_eq!(compose_url(&d), Err(MailError::BodyTooLong));
        // a body that fits the character limit but not once percent-encoded
        let d = draft(
            MailProvider::SystemDefault,
            &[],
            "",
            &"\u{e9}".repeat(MAX_BODY_CHARS),
        );
        assert_eq!(compose_url(&d), Err(MailError::LinkTooLong));
    }

    #[test]
    fn every_link_passes_the_openable_gate() {
        for p in [
            MailProvider::Gmail,
            MailProvider::Outlook,
            MailProvider::SystemDefault,
        ] {
            let d = draft(
                p,
                &["Sam@X.com"],
                "Quarterly <numbers> & more",
                "Line one.\nLine two \"quoted\".",
            );
            let url = compose_url(&d).unwrap();
            let want = if p == MailProvider::SystemDefault {
                Openable::Mailto
            } else {
                Openable::Web
            };
            assert_eq!(check_openable(&url), Ok(want), "{url}");
        }
    }

    #[test]
    fn provider_labels() {
        assert_eq!(MailProvider::Gmail.label(), "Gmail");
        assert_eq!(MailProvider::default(), MailProvider::SystemDefault);
    }
}
