//! The pre-save transform (11 §1.5; 0042 §6): what saving a wikitext page does to its text
//! before it is stored, as MediaWiki's does. Line ends become `\n` and trailing whitespace
//! is trimmed; `~~~` is the signature, `~~~~` the signature and the time, `~~~~~` the time.
//! Inside `<nowiki>`, `<pre>` and comments nothing changes. `subst:` is expanded only with
//! expansion on, which is a later slice; here it stays in the text.

/// Who signs, and when.
#[derive(Debug, Clone)]
pub struct Signer<'a> {
    /// The user name the signature links to.
    pub name: &'a str,
    /// The User namespace's local name.
    pub user_ns: &'a str,
    /// The User talk namespace's local name.
    pub user_talk_ns: &'a str,
    /// The word the talk link shows (`talk`).
    pub talk: &'a str,
    /// The time, in seconds since the epoch.
    pub now: i64,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The time as MediaWiki's default signature writes it: `15:57, 31 August 2025 (UTC)`.
#[must_use]
pub fn signature_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let name = MONTHS[usize::try_from(month - 1).unwrap_or(0)];
    format!(
        "{:02}:{:02}, {day} {name} {year} (UTC)",
        rem / 3600,
        rem % 3600 / 60
    )
}

/// Applies the pre-save transform.
#[must_use]
pub fn pre_save_transform(text: &str, signer: &Signer<'_>) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let sig = format!(
        "[[{}:{n}|{n}]] ([[{}:{n}|{}]])",
        signer.user_ns,
        signer.user_talk_ns,
        signer.talk,
        n = signer.name
    );
    let time = signature_time(signer.now);
    let mut out = String::with_capacity(text.len());
    let lower = text.to_ascii_lowercase();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let skip_to = if rest.starts_with("<!--") {
            Some(rest.find("-->").map_or(text.len(), |e| i + e + 3))
        } else {
            ["nowiki", "pre"].iter().find_map(|tag| {
                let open = format!("<{tag}");
                lower[i..].starts_with(&open).then(|| {
                    let close = format!("</{tag}>");
                    lower[i..]
                        .find(&close)
                        .map_or(text.len(), |e| i + e + close.len())
                })
            })
        };
        if let Some(end) = skip_to {
            out.push_str(&text[i..end]);
            i = end;
            continue;
        }
        if rest.starts_with("~~~") {
            let n = rest.bytes().take_while(|&b| b == b'~').count();
            match n {
                3 => out.push_str(&sig),
                4 => {
                    out.push_str(&sig);
                    out.push(' ');
                    out.push_str(&time);
                }
                _ => {
                    out.push_str(&"~".repeat(n - 5));
                    out.push_str(&time);
                }
            }
            i += n;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> Signer<'static> {
        Signer {
            name: "Harej",
            user_ns: "User",
            user_talk_ns: "User talk",
            talk: "talk",
            now: 1_756_655_820, // 2025-08-31T15:57:00Z
        }
    }

    #[test]
    fn signatures() {
        assert_eq!(signature_time(1_756_655_820), "15:57, 31 August 2025 (UTC)");
        assert_eq!(signature_time(0), "00:00, 1 January 1970 (UTC)");
        assert_eq!(signature_time(951_782_400), "00:00, 29 February 2000 (UTC)");
        assert_eq!(
            pre_save_transform("hi ~~~~\r\n\n", &signer()),
            "hi [[User:Harej|Harej]] ([[User talk:Harej|talk]]) 15:57, 31 August 2025 (UTC)"
        );
        assert_eq!(
            pre_save_transform("~~~", &signer()),
            "[[User:Harej|Harej]] ([[User talk:Harej|talk]])"
        );
        assert_eq!(
            pre_save_transform("~~~~~", &signer()),
            "15:57, 31 August 2025 (UTC)"
        );
        assert_eq!(
            pre_save_transform(
                "<nowiki>~~~~</nowiki> <!-- ~~~ --> <pre>~~~</pre>",
                &signer()
            ),
            "<nowiki>~~~~</nowiki> <!-- ~~~ --> <pre>~~~</pre>"
        );
    }
}
