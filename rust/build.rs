// Embeds Chroma's XML lexer definitions (assets/chroma) for the code block highlighter.
use std::fmt::Write;

fn main() {
    let dir = std::path::Path::new("assets/chroma");
    println!("cargo:rerun-if-changed=assets/chroma");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".xml"))
        .collect();
    names.sort();
    let mut out = String::from("pub static EMBEDDED: &[(&str, &str)] = &[\n");
    for n in names {
        writeln!(out, "    ({n:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/chroma/{n}\"))),").unwrap();
    }
    out.push_str("];\n");
    let dest = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("chroma_lexers.rs");
    std::fs::write(dest, out).unwrap();
}
