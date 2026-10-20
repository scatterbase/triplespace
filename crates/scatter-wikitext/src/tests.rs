//! Rendering tests for single constructs. The golden tests against Librarybase's own
//! renders are in `tests/golden.rs`.

use super::*;
use crate::html::url_encode_title;

struct Fixture;

impl Resolver for Fixture {
    fn info(&self, t: &Title) -> PageInfo {
        PageInfo {
            exists: !t.text.starts_with("Missing"),
            redirect: t.text.starts_with("Redirect"),
            label: (t.namespace == 120).then(|| format!("label of {}", t.text)),
        }
    }
    fn url(&self, t: &Title) -> String {
        format!("/wiki/{}", url_encode_title(&t.prefixed(&config())))
    }
    fn missing_url(&self, t: &Title) -> String {
        format!(
            "/w/index.php?title={}&action=edit&redlink=1",
            url_encode_title(&t.prefixed(&config()))
        )
    }
}

fn config() -> Config {
    let mut s = ConfigSource::default();
    s.namespaces.push(Namespace::new(120, "Item"));
    Config::new(s)
}

fn r(text: &str) -> String {
    let c = config();
    let page = Title::new(4, "Test");
    let html = render(text, &c, &Fixture, &page).html;
    html.strip_prefix("<div class=\"mw-parser-output\">")
        .and_then(|h| h.strip_suffix("</div>"))
        .unwrap_or(&html)
        .replace('\n', "")
}

#[test]
fn paragraphs() {
    assert_eq!(r("a\nb"), "<p>ab</p>");
    assert_eq!(r("a\n\nb"), "<p>a</p><p>b</p>");
    assert_eq!(r("a\n\n\nb"), "<p>a</p><p><br>b</p>");
    assert_eq!(r("<div>x</div>\ny"), "<div>x</div><p>y</p>");
    assert_eq!(r("<div>\nx\n</div>"), "<div><p>x</p></div>");
}

#[test]
fn apostrophes() {
    // Five then five: italic outside, as MediaWiki writes it.
    assert_eq!(
        r("''a'' '''b''' '''''c'''''"),
        "<p><i>a</i> <b>b</b> <i><b>c</b></i></p>"
    );
    assert_eq!(r("'''''a''' b''"), "<p><i><b>a</b> b</i></p>");
    assert_eq!(r("'''''a'' b'''"), "<p><b><i>a</i> b</b></p>");
    assert_eq!(r("''unclosed"), "<p><i>unclosed</i></p>");
    assert_eq!(r("l'''amour'' x"), "<p>l'<i>amour</i> x</p>");
    assert_eq!(r("''''x''''"), "<p>'<b>x'</b></p>");
}

#[test]
fn links() {
    assert_eq!(
        r("[[Project:Scope]] [[Missing page|m]] [[Redirect x]]s"),
        "<p><a href=\"/wiki/Project:Scope\" title=\"Project:Scope\">Project:Scope</a> \
         <a href=\"/w/index.php?title=Missing_page&amp;action=edit&amp;redlink=1\" class=\"new\" title=\"Missing page (page does not exist)\">m</a> \
         <a href=\"/wiki/Redirect_x\" class=\"mw-redirect\" title=\"Redirect x\">Redirect xs</a></p>"
    );
    assert_eq!(
        r("[[Item:Q5]] [[Item:Q5|own]]"),
        "<p><a href=\"/wiki/Item:Q5\" title=\"Item:Q5\">label of Q5</a> <a href=\"/wiki/Item:Q5\" title=\"Item:Q5\">own</a></p>"
    );
    assert_eq!(
        r("[[Project:Test]] [[#Sec]]"),
        "<p><a class=\"mw-selflink selflink\">Project:Test</a> <a href=\"#Sec\">#Sec</a></p>"
    );
    assert_eq!(r("[[a<b]]"), "<p>[[a&lt;b]]</p>");
}

#[test]
fn external_links() {
    assert_eq!(
        r("[https://a.org A] [https://b.org] see https://c.org/x."),
        "<p><a rel=\"nofollow\" class=\"external text\" href=\"https://a.org\">A</a> \
         <a rel=\"nofollow\" class=\"external autonumber\" href=\"https://b.org\">[1]</a> \
         see <a rel=\"nofollow\" class=\"external free\" href=\"https://c.org/x\">https://c.org/x</a>.</p>"
    );
}

#[test]
fn lists_and_headings() {
    assert_eq!(
        r("*a\n**b\n*c"),
        "<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>"
    );
    assert_eq!(r("#a\n#b"), "<ol><li>a</li><li>b</li></ol>");
    assert_eq!(r(";t:d"), "<dl><dt>t</dt><dd>d</dd></dl>");
    assert_eq!(
        r("== A b! =="),
        "<h2><span id=\"A_b.21\"></span><span class=\"mw-headline\" id=\"A_b!\">A b!</span></h2>"
    );
    assert_eq!(
        r("==A==\n==A=="),
        "<h2><span class=\"mw-headline\" id=\"A\">A</span></h2><h2><span class=\"mw-headline\" id=\"A_2\">A</span></h2>"
    );
}

#[test]
fn toc() {
    let html = r("x\n==A==\n===B===\n==C==\n==D==");
    assert!(html.starts_with("<p>x</p><div id=\"toc\""), "{html}");
    assert!(html.contains("<li class=\"toclevel-2 tocsection-2\"><a href=\"#B\"><span class=\"tocnumber\">1.1</span> <span class=\"toctext\">B</span></a></li>"), "{html}");
    assert!(!r("__NOTOC__\n==A==\n==B==\n==C==\n==D==").contains("toc"));
    assert!(r("==A==\n__TOC__").contains("id=\"toc\""));
}

#[test]
fn tables() {
    assert_eq!(
        r(
            "{| class=\"wikitable\" onclick=\"x\"\n|+ Cap\n! H1 !! H2\n|- style=\"color:red\"\n| a || b\nc\n|}"
        ),
        "<table class=\"wikitable\"><caption>Cap</caption><tbody><tr><th>H1</th><th>H2</th></tr>\
         <tr style=\"color:red\"><td>a</td><td>b<p>c</p></td></tr></tbody></table>"
    );
}

#[test]
fn html_and_extension_tags() {
    assert_eq!(
        r("<span style=\"background:url(x)\" onclick=\"y\">s</span>"),
        "<p><span style=\"/* insecure input */\">s</span></p>"
    );
    assert_eq!(r("<nowiki>''x'' [[y]]</nowiki>"), "<p>''x'' [[y]]</p>");
    assert_eq!(r("<pre>a ''b''</pre>"), "<pre>a ''b''</pre>");
    assert_eq!(
        r("<script>x</script>"),
        "<p>&lt;script&gt;x&lt;/script&gt;</p>"
    );
    assert_eq!(
        r("a<noinclude>b</noinclude><includeonly>c</includeonly>"),
        "<p>ab</p>"
    );
}

#[test]
fn chips() {
    assert_eq!(
        r("{{Tpl|x}}"),
        "<p><span class=\"ts-chip ts-chip--wikitext ts-chip--template\" title=\"Template not rendered\"><code>{{Tpl|x}}</code></span></p>"
    );
    let rendered = render("{{Tpl}}", &config(), &Fixture, &Title::new(4, "T"));
    assert_eq!(rendered.extracted.templates[0].name, "Tpl");
}

#[test]
fn redirects_and_categories() {
    let c = config();
    let page = Title::new(4, "T");
    let out = render(
        "#REDIRECT [[Project:Scope]]\n[[Category:X|k]]",
        &c,
        &Fixture,
        &page,
    );
    assert_eq!(out.extracted.redirect, Some(Title::new(4, "Scope")));
    assert_eq!(out.extracted.categories[0].name, "X");
    assert!(out.html.contains("<div class=\"redirectMsg\"><p>Redirect to:</p><ul class=\"redirectText\"><li><a href=\"/wiki/Project:Scope\" title=\"Project:Scope\">Project:Scope</a></li></ul></div>"), "{}", out.html);
}

#[test]
fn preformatted() {
    assert_eq!(r(" a ''b''\n c"), "<pre>a <i>b</i>c</pre>");
}
