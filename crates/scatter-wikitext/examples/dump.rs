//! Prints scatter-wikitext-parse's tree for a file: `cargo run --example dump -- page.wiki`.
fn main() {
    let path = std::env::args().nth(1).expect("a file");
    let text = std::fs::read_to_string(path).expect("readable");
    let config = scatter_wikitext_parse::Configuration::default();
    let out = config.parse_without_timeout(&text).expect("parses");
    println!("{:#?}", out.nodes);
}
