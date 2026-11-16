-- The title index in byte order (09 §4.4; 0056 A19): the hidden set finds a confidential
-- page's subpages as a range of titles, `Root/` up to the character after `/`, one index
-- scan per root. The range is a prefix only under byte-order comparison, which a locale
-- collation does not give (it weighs punctuation last), so the index and the query both
-- say `COLLATE "C"`. Every page, deleted ones included: a deleted subpage of a
-- confidential page is as absent for outsiders as the page is.

CREATE INDEX IF NOT EXISTS page_title_bytes ON view.page (tenant, ns, (title COLLATE "C"));
