//! The splitter's rules, one case each (0090 §1–§4, §6, §9).

use scatter_talk::{
    AuthorSource, Blame, Conversion, Fallback, Markup, Options, PostKind, Revision, Site, Stamp,
    ThreadOrigin, attribute, convert,
};

fn run(text: &str) -> Conversion {
    convert(
        text,
        Markup::embedded(),
        &Site::english(),
        &Options::default(),
    )
}

/// (kind, depth, parent, signer) of each post of thread `t`.
fn shape(c: &Conversion, t: usize) -> Vec<(String, usize, Option<usize>, Option<String>)> {
    c.threads[t]
        .posts
        .iter()
        .map(|p| {
            let kind = match &p.kind {
                PostKind::Comment => "comment".to_string(),
                PostKind::Unsigned => "unsigned".to_string(),
                PostKind::Heading { level } => format!("h{level}"),
                PostKind::Closing { .. } => "closing".to_string(),
                PostKind::Collapse { first, last, .. } => format!("collapse:{first}-{last}"),
                PostKind::Section(f) => format!("section:{}", f.reason()),
                PostKind::Page(f) => format!("page:{}", f.reason()),
            };
            (
                kind,
                p.depth,
                p.parent,
                p.signature.as_ref().map(|s| s.user.clone()),
            )
        })
        .collect()
}

fn c(
    kind: &str,
    depth: usize,
    parent: Option<usize>,
    who: Option<&str>,
) -> (String, usize, Option<usize>, Option<String>) {
    (kind.to_string(), depth, parent, who.map(str::to_string))
}

#[test]
fn comments_replies_and_indentation() {
    let page = "== Merge? ==\nShould we merge?\nI think so. [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:No. [[User:B|B]] 12:05, 5 June 2025 (UTC)\n::Why not? [[User:A|A]] 12:10, 5 June 2025 (UTC)\n:Agree with B. [[User:C|C]] 12:20, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(conv.threads.len(), 1);
    assert_eq!(conv.threads[0].subject, "Merge?");
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("comment", 1, Some(0), Some("B")),
            c("comment", 2, Some(1), Some("A")),
            c("comment", 1, Some(0), Some("C"))
        ]
    );
    // Unsigned text before a comment is part of it; each post loses its own indentation.
    assert_eq!(
        conv.threads[0].posts[0].text,
        "Should we merge?\nI think so. [[User:A|A]] 12:00, 5 June 2025 (UTC)"
    );
    assert_eq!(
        conv.threads[0].posts[2].text,
        "Why not? [[User:A|A]] 12:10, 5 June 2025 (UTC)"
    );
}

#[test]
fn outdent_replies_to_the_previous_comment() {
    let page = "== X ==\nA [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:::::B [[User:B|B]] 12:05, 5 June 2025 (UTC)\n{{od|:::::}}C [[User:C|C]] 12:10, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(shape(&conv, 0)[2], c("comment", 0, Some(1), Some("C")));
}

#[test]
fn front_matter_banners_and_comments_before_the_first_heading() {
    let page = "{{Talk header}}\n{{WikiProject Biography\n|class=B}}\n\nThis article is too short. --[[User:LMS|LMS]]\n:Agreed. [[User:B|B]] 12:05, 5 June 2004 (UTC)\n== Later ==\nHi [[User:C|C]] 12:00, 5 June 2005 (UTC)";
    let conv = run(page);
    let header = conv.header.as_ref().expect("a header");
    assert_eq!(
        header.text,
        "{{Talk header}}\n{{WikiProject Biography\n|class=B}}"
    );
    assert_eq!(conv.threads[0].origin, ThreadOrigin::BeforeFirstHeading);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("LMS")),
            c("comment", 1, Some(0), Some("B"))
        ]
    );
    assert_eq!(conv.threads[1].subject, "Later");
}

#[test]
fn banners_alone_stay_the_header() {
    let conv =
        run("{{Talk header}}\n{{Archives}}\n== A ==\nHi [[User:C|C]] 12:00, 5 June 2005 (UTC)");
    assert!(conv.header.is_some());
    assert_eq!(conv.threads.len(), 1);
}

#[test]
fn level_one_headings_are_not_structure() {
    let page = "= October 7 =\n\n== Why? ==\nQ [[User:A|A]] 12:00, 7 October 2026 (UTC)\n= October 8 =\n== Mars ==\nQ2 [[User:B|B]] 12:00, 8 October 2026 (UTC)";
    let conv = run(page);
    let subjects: Vec<&str> = conv.threads.iter().map(|t| t.subject.as_str()).collect();
    assert_eq!(subjects, ["Why?", "Mars"]);
    // DiscussionTools still names the dropped heading in the thread heading's ID.
    assert_eq!(
        conv.threads[0].dt.as_ref().map(|d| d.id.as_str()),
        Some("h-Why?-October_7-20261007120000")
    );
}

#[test]
fn subsection_headings_become_heading_posts() {
    let page = "== RfA ==\nIntro [[User:N|N]] 12:00, 1 October 2026 (UTC)\n===== Support =====\n# Yes [[User:A|A]] 12:01, 1 October 2026 (UTC)\n#: Why? [[User:Q|Q]] 12:02, 1 October 2026 (UTC)\n===== Oppose =====\n# No [[User:B|B]] 12:03, 1 October 2026 (UTC)";
    let conv = run(page);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("N")),
            c("h5", 0, None, None),
            c("comment", 1, Some(1), Some("A")),
            c("comment", 2, Some(2), Some("Q")),
            c("h5", 0, None, None),
            c("comment", 1, Some(4), Some("B")),
        ]
    );
    assert_eq!(conv.threads[0].posts[1].text, "Support");
}

#[test]
fn a_wrapper_closing_the_whole_section_is_a_closure() {
    let page = "== Dispute ==\n{{atop|result=Blocked. [[User:Admin|Admin]] 13:00, 5 June 2025 (UTC)}}\nReport. [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:Reply. [[User:B|B]] 12:05, 5 June 2025 (UTC)\n{{abot}}";
    let conv = run(page);
    assert_eq!(conv.threads[0].fallback, None);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("comment", 1, Some(0), Some("B")),
            c("closing", 0, None, Some("Admin")),
        ]
    );
    let closing = &conv.threads[0].posts[2];
    assert_eq!(
        closing.text,
        "Blocked. [[User:Admin|Admin]] 13:00, 5 June 2025 (UTC)"
    );
    let PostKind::Closing { wrapper } = &closing.kind else {
        panic!()
    };
    assert_eq!(wrapper[1], "{{abot}}");
}

#[test]
fn a_wrapper_around_part_of_a_section_collapses_it() {
    let page = "== Dispute ==\nReport. [[User:A|A]] 12:00, 5 June 2025 (UTC)\n{{hat|Off topic}}\n:Tangent. [[User:B|B]] 12:05, 5 June 2025 (UTC)\n::More. [[User:C|C]] 12:06, 5 June 2025 (UTC)\n{{hab}}\n:Back. [[User:D|D]] 12:10, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(conv.threads[0].fallback, None);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("comment", 1, Some(0), Some("B")),
            c("comment", 2, Some(1), Some("C")),
            c("collapse:1-2", 0, None, None),
            c("comment", 1, Some(0), Some("D")),
        ]
    );
    assert_eq!(conv.threads[0].posts[3].text, "Off topic");
}

#[test]
fn an_unclosed_wrapper_collapses_to_the_end_of_its_section() {
    let page = "== X ==\nA [[User:A|A]] 12:00, 5 June 2025 (UTC)\n{{hat|reason=Trolling}}\n:B [[User:B|B]] 12:05, 5 June 2025 (UTC)\n== Y ==\nC [[User:C|C]] 12:10, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(shape(&conv, 0)[2], c("collapse:1-1", 0, None, None));
    assert_eq!(conv.threads[0].posts[2].text, "Trolling");
    assert_eq!(conv.threads[1].posts.len(), 1);
}

#[test]
fn a_wrapper_around_a_subsection_collapses_it_under_its_heading() {
    let page = "== RfC ==\nQ [[User:A|A]] 12:00, 5 June 2025 (UTC)\n=== Discussion ===\n{{collapse top|title=Side argument}}\n:B [[User:B|B]] 12:05, 5 June 2025 (UTC)\n{{collapse bottom}}\n=== Votes ===\n* Yes [[User:C|C]] 12:10, 5 June 2025 (UTC)";
    let conv = run(page);
    let s = shape(&conv, 0);
    assert_eq!(s[1], c("h3", 0, None, None));
    assert_eq!(s[3], c("collapse:2-2", 0, Some(1), None));
    assert_eq!(conv.threads[0].posts[3].text, "Side argument");
}

#[test]
fn a_wrapper_inside_one_comment_is_harmless() {
    let page = "== X ==\n:::{{cot|reason=Long}}\n:::Detail.\n{{cob}}\n:::So there. [[User:A|A]] 12:00, 5 June 2025 (UTC)\n::::Ok. [[User:B|B]] 12:05, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(conv.threads[0].fallback, None);
    assert_eq!(conv.threads[0].posts.len(), 2);
}

#[test]
fn unsigned_text_before_a_wrapper_is_its_own_post() {
    let page = "== X ==\nA [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:An unsigned aside.\n{{hat|Tangent}}\n:B [[User:B|B]] 12:05, 5 June 2025 (UTC)\n{{hab}}";
    let conv = run(page);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("unsigned", 1, Some(0), None),
            c("comment", 1, Some(0), Some("B")),
            c("collapse:2-2", 0, None, None),
        ]
    );
    assert_eq!(
        conv.threads[0].posts[2].text,
        "B [[User:B|B]] 12:05, 5 June 2025 (UTC)"
    );
}

#[test]
fn a_wrapper_inside_one_comment_keeps_the_text_before_it() {
    let page = "== X ==\n:The evidence:\n{{cot|Evidence}}\n:Detail.\n{{cob}}\n:So there. [[User:A|A]] 12:00, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(shape(&conv, 0), [c("comment", 1, None, Some("A"))]);
    assert!(
        conv.threads[0].posts[0]
            .text
            .starts_with("The evidence:\n{{cot|Evidence}}")
    );
}

#[test]
fn a_signature_inside_a_box_runs_on_to_its_end() {
    let page = "== Welcome! ==\n<div style=\"border: 1px solid gray\">\nWelcome to Wikipedia! [[User:A|A]] 12:00, 5 June 2025 (UTC)\n</div>\n:Thanks! [[User:J|J]] 12:30, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("comment", 1, Some(0), Some("J"))
        ]
    );
    assert!(conv.threads[0].posts[0].text.ends_with("</div>"));
}

#[test]
fn a_signature_inside_a_table_runs_on_to_its_end() {
    let page = "== A barnstar for you! ==\n{| style=\"border: 1px solid gray\"\n|rowspan=\"2\" | [[File:Star.png|100px]]\n|style=\"font-size: x-large\" | '''The Barnstar'''\n|-\n| Thanks! [[User:A|A]] 12:00, 5 June 2025 (UTC)\n|}\n:Thank you! [[User:J|J]] 12:30, 5 June 2025 (UTC)";
    let conv = run(page);
    assert_eq!(conv.threads[0].fallback, None);
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("comment", 1, Some(0), Some("J"))
        ]
    );
    assert!(conv.threads[0].posts[0].text.ends_with("|}"));
}

#[test]
fn a_table_holding_two_signatures_falls_back() {
    let page = "== Votes ==\n{|\n| A [[User:A|A]] 12:00, 5 June 2025 (UTC)\n|-\n| B [[User:B|B]] 12:01, 5 June 2025 (UTC)\n|}";
    assert_eq!(
        run(page).threads[0].fallback,
        Some(Fallback::Construct("open-table"))
    );
}

#[test]
fn a_section_without_signatures_falls_back() {
    let conv = run("== Notes ==\nSome notes.\nMore notes.");
    assert_eq!(shape(&conv, 0), [c("section:no-signature", 0, None, None)]);
    assert_eq!(conv.threads[0].posts[0].text, "Some notes.\nMore notes.");
}

#[test]
fn a_page_too_large_falls_back_whole() {
    let opts = Options {
        max_bytes: 10,
        ..Options::default()
    };
    let conv = convert(
        "== A ==\nlong text here",
        Markup::embedded(),
        &Site::english(),
        &opts,
    );
    assert_eq!(conv.fallback, Some(Fallback::TooLarge));
    assert_eq!(conv.threads[0].origin, ThreadOrigin::Page);
}

#[test]
fn trailing_unsigned_text_is_its_own_post() {
    let conv = run("== X ==\nA [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:A late unsigned reply.");
    assert_eq!(
        shape(&conv, 0),
        [
            c("comment", 0, None, Some("A")),
            c("unsigned", 1, Some(0), None)
        ]
    );
}

#[test]
fn discussiontools_names_and_ids() {
    let conv = run(
        "== Hello ==\nHi [[User:Some One|Some One]] 12:00, 5 June 2025 (UTC)\n:Re [[User:B|B]] 12:05, 5 June 2025 (UTC)\n:Re [[User:B|B]] 12:05, 5 June 2025 (UTC)",
    );
    let t = &conv.threads[0];
    let ids: Vec<(&str, &str)> = t
        .posts
        .iter()
        .filter_map(|p| p.dt.as_ref())
        .map(|d| (d.name.as_str(), d.id.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            (
                "c-Some_One-20250605120000",
                "c-Some_One-20250605120000-Hello"
            ),
            (
                "c-B-20250605120500",
                "c-B-20250605120500-Some_One-20250605120000"
            ),
            (
                "c-B-20250605120500",
                "c-B-20250605120500-Some_One-20250605120000-1"
            ),
        ]
    );
    let h = t.dt.as_ref().expect("heading ids");
    assert_eq!(
        (h.name.as_str(), h.id.as_str()),
        ("h-Some_One-20250605120000", "h-Hello-20250605120000")
    );
    let old = run("== Hi ==\nX [[User:A|A]] 07:50, 10 July 2006 (UTC)");
    assert_eq!(
        old.threads[0].posts[0].dt.as_ref().map(|d| d.id.as_str()),
        Some("c-A-2006-07-10T07:50:00.000Z-Hi")
    );
}

fn stamp(d: i64, h: i64, mi: i64) -> Stamp {
    Stamp::from_civil(2005, 6, d, h, mi).expect("a date")
}

#[test]
fn attribution_from_signature_and_history() {
    let (markup, site) = (Markup::embedded(), Site::english());
    let mut blame = Blame::new(markup, &site);
    let r = |id, actor: &str, d| Revision {
        id,
        actor: actor.to_string(),
        timestamp: stamp(d, 12, 0),
    };
    // A renamed user: history says "New", signatures say "Old".
    let v1 =
        "== A ==\nFirst point, which is long enough. [[User:Old|Old]] 12:00, 1 June 2005 (UTC)";
    let v2 =
        format!("{v1}\n:Second point, also long enough. [[User:Old|Old]] 12:00, 2 June 2005 (UTC)");
    // An unsigned addition by someone else, then a comment signed by SineBot for them.
    let v3 = format!(
        "{v2}\n:An unsigned remark by a passer-by\n:and more. [[User:Passer|Passer]] 12:00, 3 June 2005 (UTC)"
    );
    blame.push(r(1, "New", 1), v1);
    blame.push(r(2, "New", 2), &v2);
    blame.push(r(3, "Passer", 3), &v3);
    assert!(blame.is_alias("New", "Old"));
    let mut conv = convert(&v3, markup, &site, &Options::default());
    attribute(&mut conv, &v3, Some(&blame), "Importer", markup, &site);
    let got: Vec<(String, AuthorSource)> = conv.threads[0]
        .posts
        .iter()
        .map(|p| {
            let a = p.attribution.as_ref().expect("attributed");
            (a.author.clone(), a.source)
        })
        .collect();
    assert_eq!(
        got,
        [
            ("New".into(), AuthorSource::History),
            ("New".into(), AuthorSource::History),
            ("Passer".into(), AuthorSource::History),
        ]
    );
    // Without history, the signature decides.
    let mut plain = convert(&v3, markup, &site, &Options::default());
    attribute(&mut plain, &v3, None, "Importer", markup, &site);
    let a = plain.threads[0].posts[0]
        .attribution
        .as_ref()
        .expect("attributed");
    assert_eq!(
        (a.author.as_str(), a.source),
        ("Old", AuthorSource::Signature)
    );
}

#[test]
fn history_splits_text_someone_else_added() {
    let (markup, site) = (Markup::embedded(), Site::english());
    let mut blame = Blame::new(markup, &site);
    let r = |id, actor: &str| Revision {
        id,
        actor: actor.to_string(),
        timestamp: stamp(4, 12, 0),
    };
    let v1 = "== A ==\nA question somebody asked without signing it, long enough.";
    let v2 =
        format!("{v1}\nAn answer from someone who did sign. [[User:B|B]] 12:00, 4 June 2005 (UTC)");
    blame.push(r(1, "Asker"), v1);
    blame.push(r(2, "B"), &v2);
    let mut conv = convert(&v2, markup, &site, &Options::default());
    assert_eq!(
        conv.threads[0].posts.len(),
        1,
        "without history the unsigned text joins the comment"
    );
    attribute(&mut conv, &v2, Some(&blame), "Importer", markup, &site);
    let posts = &conv.threads[0].posts;
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].kind, PostKind::Unsigned);
    assert_eq!(
        posts[0].attribution.as_ref().map(|a| a.author.as_str()),
        Some("Asker")
    );
    assert_eq!(
        posts[1].attribution.as_ref().map(|a| a.author.as_str()),
        Some("B")
    );
}
