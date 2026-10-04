//! The entity page's parts as HTML: the identity line, the terms, the tabs that hold
//! statement groups (drawn by [`super::shapes`]), the Identifiers, Sitelinks and Labels
//! tabs, and "Where this comes from".

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use scatter_wikibase_model::id::IdForm;
use scatter_wikibase_model::sites::SiteRegistry;
use scatter_wikibase_model::statement::{Reference, SnakGroups, Statement};

use super::Render;
use crate::codex::{Accordion, Align, Cell, Chip, ChipKind, Column, Table};
use crate::frame::title_url;
use crate::html::{esc, lang_attrs, link};

fn chip(kind: ChipKind, text: &str) -> String {
    Chip::new(kind, text).render().unwrap_or_default()
}

impl Render<'_> {
    fn pick<'m>(&self, map: &'m BTreeMap<String, String>) -> Option<(&'m str, &'m str)> {
        self.m
            .term_chain()
            .into_iter()
            .find_map(|l| map.get_key_value(&l))
            .map(|(l, t)| (t.as_str(), l.as_str()))
    }

    /// The label in the reader's term chain, with its language.
    #[must_use]
    pub fn label(&self) -> Option<(&str, &str)> {
        self.pick(&self.entity.labels)
    }

    /// The title as shown: the label, else the ID, or for a keyed entity its key.
    #[must_use]
    pub fn display_title(&self) -> String {
        if let Some((t, _)) = self.label() {
            return t.to_string();
        }
        match self.entity.id.keyed_parts() {
            Some((_, key)) => key.to_string(),
            None => self.entity.id.as_str().to_string(),
        }
    }

    /// The document title: the label with the ID, as Wikibase shows it.
    #[must_use]
    pub fn document_title(&self) -> String {
        match self.label() {
            Some((t, _)) if self.entity.id.form() != IdForm::Keyed => {
                format!("{t} ({})", self.entity.id.as_str())
            }
            Some((t, _)) => t.to_string(),
            None => self.display_title(),
        }
    }

    fn provider_name(&self, code: &str) -> String {
        self.site
            .providers
            .iter()
            .find(|p| p.code == code)
            .map_or_else(|| code.to_string(), |p| p.name.clone())
    }

    fn concept_iri(&self) -> Option<String> {
        let id = &self.entity.id;
        match id.form() {
            IdForm::Local if !self.site.concept_base.is_empty() => {
                Some(format!("{}{}", self.site.concept_base, id.as_str()))
            }
            IdForm::Foreign => {
                let s = id.as_str();
                let (code, rest) = (&s[..2], &s[2..]);
                let type_code = rest.chars().next()?.to_string();
                let p = self.site.providers.iter().find(|p| p.code == code)?;
                let t = p.types.iter().find(|t| t.code == type_code)?;
                let upstream = self
                    .provenance
                    .and_then(|pr| pr.minted_by.upstream_id.clone())
                    .unwrap_or_else(|| format!("{}{}", t.upstream_prefix, &rest[1..]));
                Some(t.iri.replace("{upstream_id}", &upstream))
            }
            _ => None,
        }
    }

    /// The identity line (0010 §2): a chip for the ID or namespace, the kind of page,
    /// where it comes from, and the concept IRI.
    #[must_use]
    pub fn identity(&self) -> String {
        let id = &self.entity.id;
        let et = self.entity.entity_type.name();
        let (chip_html, kind) = match id.form() {
            IdForm::Local => {
                let kind = match (et, &self.entity.datatype) {
                    ("property", Some(dt)) => self
                        .m
                        .with("ts-kind-property-datatype", &[&self.datatype_name(dt.id())]),
                    ("property", None) => self.m.get("ts-kind-property"),
                    _ => self.m.get("ts-kind-item"),
                };
                (chip(ChipKind::Local, id.as_str()), kind)
            }
            IdForm::Foreign => {
                let code = &id.as_str()[..2];
                let name = self.provider_name(code);
                let key = match et {
                    "item" => "ts-kind-foreign-item",
                    "property" => "ts-kind-foreign-property",
                    _ => "ts-kind-foreign-entity",
                };
                (
                    chip(ChipKind::Provider(code.to_string()), id.as_str()),
                    self.m.with(key, &[&name]),
                )
            }
            IdForm::Keyed => {
                let key = match et {
                    "domain" => "ts-kind-domain",
                    "keyword" => "ts-kind-keyword",
                    "notation" => "ts-kind-notation",
                    _ => "ts-kind-entity",
                };
                let k = self.m.get(key);
                // The key is the ID; the title shows the label, so the key is shown here.
                let name = id.keyed_parts().map_or("", |(_, key)| key);
                (
                    format!(
                        "{} <span class=\"ts-mono\" dir=\"ltr\">{}</span>",
                        chip(ChipKind::Namespace, &k),
                        esc(name)
                    ),
                    self.m.get("ts-kind-keyed"),
                )
            }
        };
        let mut s = format!("{chip_html} <span>{}</span>", esc(&kind));
        if let Some(from) = self
            .provenance
            .and_then(|p| p.minted_by.adopted_from.as_deref())
        {
            let _ = write!(
                s,
                " <span aria-hidden=\"true\">·</span> <span>{}</span>",
                esc(&self.m.with("ts-adopted-from", &[from]))
            );
        }
        if let Some(iri) = self.concept_iri().filter(|i| crate::html::linkable(i)) {
            let shown = iri
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_start_matches("www.");
            let _ = write!(
                s,
                " <span aria-hidden=\"true\">·</span> {}",
                link(
                    &iri,
                    &format!("{} ↗", esc(shown)),
                    " class=\"external\" rel=\"noopener\" dir=\"ltr\""
                )
            );
        }
        s
    }

    fn datatype_name(&self, id: &str) -> String {
        let key = format!("ts-datatype-{}", id.to_ascii_lowercase());
        let msg = self.m.get(&key);
        if msg.starts_with('⧼') {
            id.to_string()
        } else {
            msg
        }
    }

    /// The `terms` region: the description and the aliases under the title.
    #[must_use]
    pub fn terms(&self) -> String {
        let mut s = String::from("<div class=\"ts-terms\" data-region=\"terms\">");
        match self.pick(&self.entity.descriptions) {
            Some((d, l)) => {
                let _ = write!(
                    s,
                    "<p class=\"ts-terms__description\"{}>{}</p>",
                    lang_attrs(l, self.m.lang()),
                    esc(d)
                );
            }
            None => {
                let _ = write!(
                    s,
                    "<p class=\"ts-terms__description ts-terms__description--none\">{}</p>",
                    esc(&self.m.get("ts-terms-no-description"))
                );
            }
        }
        let aliases = self
            .m
            .term_chain()
            .into_iter()
            .find_map(|l| self.entity.aliases.get_key_value(&l));
        if let Some((l, list)) = aliases.filter(|(_, v)| !v.is_empty()) {
            let joined = list.iter().map(|a| esc(a)).collect::<Vec<_>>().join(" · ");
            let _ = write!(
                s,
                "<p class=\"ts-terms__aliases\">{} <span{}>{joined}</span></p>",
                esc(&self.m.get("ts-terms-aliases")),
                lang_attrs(l, self.m.lang())
            );
        }
        s.push_str("</div>");
        s
    }

    /// A statement's property's data type: from its main snak, else from the lookup.
    pub(super) fn datatype_of(&self, property: &str, statements: &[Statement]) -> Option<String> {
        statements
            .iter()
            .find_map(|s| s.mainsnak.datatype.as_ref().map(|d| d.id().to_string()))
            .or_else(|| self.lookup.datatype(property).map(str::to_string))
    }

    fn is_identifier(&self, property: &str, statements: &[Statement]) -> bool {
        self.datatype_of(property, statements).as_deref() == Some("external-id")
    }

    pub(super) fn property_link(&self, property: &str) -> String {
        self.id_link(property, "property")
    }

    pub(super) fn snak_groups(&self, groups: &SnakGroups, class: &str) -> String {
        let mut s = format!("<dl class=\"{class}\">");
        for (p, snaks) in groups {
            let values = snaks
                .iter()
                .map(|v| self.snak(v))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(
                s,
                "<div class=\"{class}__row\"><dt>{}</dt><dd>{values}</dd></div>",
                self.property_link(p.as_str())
            );
        }
        s.push_str("</dl>");
        s
    }

    pub(super) fn references(&self, refs: &[Reference]) -> String {
        if refs.is_empty() {
            return String::new();
        }
        let mut list = String::from("<ol class=\"ts-references\">");
        for r in refs {
            let _ = write!(list, "<li>{}</li>", self.snak_groups(&r.snaks, "ts-snaks"));
        }
        list.push_str("</ol>");
        let n = refs.len().to_string();
        Accordion {
            title: &self.m.with("ts-references", &[&n]),
            description: None,
            content: &list,
            open: false,
        }
        .render()
        .unwrap_or_default()
    }

    /// The chips for where a statement comes from: shown only where its graphs are not
    /// the entity's dominant one, so the usual case stays quiet; and "Corrected here".
    pub(super) fn statement_chips(&self, st: &Statement) -> String {
        let mut s = String::new();
        let (Some(p), Some(sid)) = (self.provenance, st.id.as_ref()) else {
            return s;
        };
        if let (Some(dominant), Some(src)) = (&p.dominant, p.statements.get(sid.as_str()))
            && !src.graphs.iter().any(|g| g == dominant)
        {
            for g in &src.graphs {
                s.push(' ');
                s.push_str(&self.graph_chip(g));
            }
        }
        if p.corrections.contains_key(sid.as_str()) {
            s.push(' ');
            s.push_str(&chip(ChipKind::Corrected, &self.m.get("ts-chip-corrected")));
        }
        s
    }

    fn graph_chip(&self, graph: &str) -> String {
        if graph == "local" {
            return chip(ChipKind::Local, &self.m.get("ts-chip-local"));
        }
        if let Some(slug) = graph.strip_prefix("mirror/")
            && let Some(p) = self.site.providers.iter().find(|p| p.slug == slug)
        {
            return chip(ChipKind::Provider(p.code.clone()), &p.code);
        }
        chip(ChipKind::Namespace, graph)
    }

    /// The groups of one tab, in property order: by provider, then by number, so that
    /// `P2` comes before `P10` (the API serves them in canonical, textual order).
    fn groups(&self, identifiers: bool) -> String {
        let mut props: Vec<&str> = self
            .entity
            .statements
            .iter()
            .filter(|(p, list)| self.is_identifier(p.as_str(), list) == identifiers)
            .map(|(p, _)| p.as_str())
            .collect();
        props.sort_by_key(|p| property_order(p));
        props.into_iter().map(|p| self.group(p)).collect()
    }

    /// The Statements tab: its groups and "Where this comes from".
    #[must_use]
    pub fn statements_tab(&self) -> String {
        let mut groups = self.groups(false);
        if groups.is_empty() {
            groups = format!(
                "<p class=\"ts-empty\">{}</p>",
                esc(&self.m.get("ts-statements-none"))
            );
        }
        format!(
            "<div class=\"ts-entity\"><div class=\"ts-entity__main\"><h2 class=\"ts-visually-hidden\">{}</h2>{groups}</div>{}</div>",
            esc(&self.m.get("ts-tab-statements")),
            self.sources()
        )
    }

    /// The `identifiers` region.
    #[must_use]
    pub fn identifiers(&self) -> String {
        let mut groups = self.groups(true);
        if groups.is_empty() {
            groups = format!(
                "<p class=\"ts-empty\">{}</p>",
                esc(&self.m.get("ts-identifiers-none"))
            );
        }
        format!("<div class=\"ts-identifiers\" data-region=\"identifiers\">{groups}</div>")
    }

    /// The `sitelinks` region: links grouped by host, each host a Domain chip linking to
    /// its Domain page (0010 §2; 0026 §9).
    #[must_use]
    pub fn sitelinks(&self) -> String {
        let registry = SiteRegistry::default_registry();
        let mut by_host: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (key, l) in &self.entity.sitelinks {
            let site = registry.by_id(&l.site).or_else(|| registry.by_id(key));
            let host = site.map_or_else(|| l.site.clone(), |s| s.host.clone());
            let url = registry
                .sitelink_url(l)
                .or_else(|| l.url.clone())
                .filter(|u| crate::html::linkable(u));
            let lang = site.and_then(|s| s.language.clone()).unwrap_or_default();
            let title = format!(
                "<span{}>{}</span>",
                lang_attrs(&lang, self.m.lang()),
                esc(&l.title)
            );
            let mut item = match url {
                Some(u) => link(&u, &title, " class=\"external\" rel=\"noopener\""),
                None => title,
            };
            for b in &l.badges {
                item.push(' ');
                item.push_str(&self.id_link(b.as_str(), "item"));
            }
            by_host.entry(host).or_default().push(item);
        }
        let mut s = String::from("<div class=\"ts-sitelinks\" data-region=\"sitelinks\">");
        if by_host.is_empty() {
            let _ = write!(
                s,
                "<p class=\"ts-empty\">{}</p>",
                esc(&self.m.get("ts-sitelinks-none"))
            );
        }
        for (host, items) in by_host {
            let href = format!("/wiki/{}", title_url(&format!("Domain:{host}")));
            let _ = write!(
                s,
                "<section class=\"ts-sitelinks__host\"><h3>{}</h3><ul>",
                link(&href, &chip(ChipKind::Namespace, &host), "")
            );
            for i in items {
                let _ = write!(s, "<li>{i}</li>");
            }
            s.push_str("</ul></section>");
        }
        s.push_str("</div>");
        s
    }

    /// The Labels tab: every language's label, description and aliases, the reader's
    /// languages first.
    #[must_use]
    pub fn labels(&self) -> String {
        let e = self.entity;
        let mut langs: Vec<String> = e
            .labels
            .keys()
            .chain(e.descriptions.keys())
            .chain(e.aliases.keys())
            .cloned()
            .collect();
        langs.sort();
        langs.dedup();
        let chain = self.m.term_chain();
        langs.sort_by_key(|l| chain.iter().position(|c| c == l).unwrap_or(usize::MAX));
        let rows = langs
            .iter()
            .map(|l| {
                let text = |o: Option<&String>| o.map(|t| esc(t)).unwrap_or_default();
                vec![
                    Cell {
                        header: true,
                        ..Cell::html(format!("<span class=\"ts-mono\">{}</span>", esc(l)))
                    },
                    Cell {
                        lang: Some(l.clone()),
                        ..Cell::html(text(e.labels.get(l)))
                    },
                    Cell {
                        lang: Some(l.clone()),
                        ..Cell::html(text(e.descriptions.get(l)))
                    },
                    Cell {
                        lang: Some(l.clone()),
                        ..Cell::html(
                            e.aliases
                                .get(l)
                                .map(|a| a.iter().map(|x| esc(x)).collect::<Vec<_>>().join(" · "))
                                .unwrap_or_default(),
                        )
                    },
                ]
            })
            .collect();
        let col = |k: &str| Column {
            label: self.m.get(k),
            align: Align::Start,
        };
        Table {
            header: None,
            caption: self.m.get("ts-labels-caption"),
            columns: vec![
                col("ts-labels-language"),
                col("ts-labels-label"),
                col("ts-labels-description"),
                col("ts-labels-aliases"),
            ],
            rows,
            empty: self.m.get("ts-labels-none"),
        }
        .render()
        .unwrap_or_default()
    }

    /// "Where this comes from" (0003 §6): each graph holding a record for the entity,
    /// with its statements and sync, and what happens if the upstream deletes it.
    #[must_use]
    pub fn sources(&self) -> String {
        let mut s = format!(
            "<aside class=\"ts-sources\" aria-labelledby=\"ts-sources-title\"><h2 class=\"ts-sources__title\" id=\"ts-sources-title\">{}</h2>",
            esc(&self.m.get("ts-sources-title"))
        );
        match self.provenance {
            None => {
                let _ = write!(
                    s,
                    "<p class=\"ts-empty\">{}</p>",
                    esc(&self.m.get("ts-sources-unknown"))
                );
            }
            Some(p) => {
                for g in &p.graphs {
                    let name = if g.graph == "local" {
                        self.m.get("ts-sources-local")
                    } else if let Some(slug) = g.graph.strip_prefix("mirror/") {
                        self.site
                            .providers
                            .iter()
                            .find(|x| x.slug == slug)
                            .map_or_else(
                                || slug.to_string(),
                                |x| self.m.with("ts-sources-mirror", &[&x.name]),
                            )
                    } else {
                        g.graph.clone()
                    };
                    let n = g.statements.to_string();
                    let _ = write!(
                        s,
                        "<section class=\"ts-sources__graph\"><h3>{} {}</h3><p>{}</p>",
                        self.graph_chip(&g.graph),
                        esc(&name),
                        esc(&self.m.with("ts-sources-statements", &[&n]))
                    );
                    if let Some(at) = &g.synced_at {
                        let mut line = self.m.with("ts-sources-synced", &[&self.date(at)]);
                        if let Some(v) = &g.upstream_version {
                            line.push_str(" · ");
                            line.push_str(&self.m.with("ts-sources-upstream", &[v]));
                        }
                        let _ = write!(s, "<p class=\"ts-subtle\">{}</p>", esc(&line));
                    }
                    s.push_str("</section>");
                }
                if let Some(r) = &p.retention {
                    let _ = write!(
                        s,
                        "<section class=\"ts-sources__graph\"><p class=\"ts-subtle\">{}</p></section>",
                        esc(&self.m.with("ts-sources-retention", &[r]))
                    );
                }
            }
        }
        let data = format!(
            "/wiki/Special:EntityData/{}.json",
            title_url(self.entity.id.as_str())
        );
        let _ = write!(
            s,
            "<p class=\"ts-sources__actions\">{}</p></aside>",
            link(&data, &esc(&self.m.get("ts-sources-json")), "")
        );
        s
    }

    /// A date from an ISO 8601 timestamp, as the reader's language writes it.
    fn date(&self, iso: &str) -> String {
        let d = iso.split('T').next().unwrap_or(iso);
        let mut it = d.split('-');
        match (
            it.next(),
            it.next().and_then(|m| m.parse::<u32>().ok()),
            it.next().and_then(|x| x.parse::<u32>().ok()),
        ) {
            (Some(y), Some(mo), Some(day)) if (1..=12).contains(&mo) => self.m.with(
                "ts-time-day",
                &[&day.to_string(), &self.m.get(&format!("ts-month-{mo}")), y],
            ),
            _ => iso.to_string(),
        }
    }
}

/// The sort key of a property ID: its prefix (empty for a local ID), then its number.
fn property_order(id: &str) -> (String, u64, String) {
    let digits = id.trim_start_matches(|c: char| !c.is_ascii_digit());
    let prefix = &id[..id.len() - digits.len()];
    let prefix = prefix.strip_suffix('P').unwrap_or(prefix);
    (
        prefix.to_string(),
        digits.parse().unwrap_or(u64::MAX),
        id.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::property_order;

    #[test]
    fn properties_sort_by_number() {
        let mut v = vec!["P10", "WDP31", "P2", "P12", "WDP5"];
        v.sort_by_key(|p| property_order(p));
        assert_eq!(v, vec!["P2", "P10", "P12", "WDP5", "WDP31"]);
    }
}
