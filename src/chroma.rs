//! A port of the parts of Chroma (github.com/alecthomas/chroma/v2) that glamour
//! uses to highlight code blocks: the XML regex lexers (embedded from
//! assets/chroma), the hand-written Go and Markdown lexers, the lexer registry
//! (Get / Match / Analyse) and the token-type hierarchy.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

use fancy_regex::{Regex, RegexBuilder};

include!(concat!(env!("OUT_DIR"), "/chroma_lexers.rs"));

pub type TokenType = i32;

pub const ERROR: TokenType = -11;
pub const OTHER: TokenType = -12;
pub const IGNORE: TokenType = -14;
pub const TEXT: TokenType = 8000;

fn token_types() -> &'static HashMap<&'static str, TokenType> {
    static T: OnceLock<HashMap<&'static str, TokenType>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        let mut add = |base: TokenType, names: &[&'static str]| {
            for (i, n) in names.iter().enumerate() {
                m.insert(*n, base + i as TokenType);
            }
        };
        add(-11, &["Error", "Other", "None", "Ignore"]);
        add(
            1000,
            &[
                "Keyword",
                "KeywordConstant",
                "KeywordDeclaration",
                "KeywordNamespace",
                "KeywordPseudo",
                "KeywordReserved",
                "KeywordType",
            ],
        );
        add(
            2000,
            &[
                "Name",
                "NameAttribute",
                "NameClass",
                "NameConstant",
                "NameDecorator",
                "NameEntity",
                "NameException",
                "NameKeyword",
                "NameLabel",
                "NameNamespace",
                "NameOperator",
                "NameOther",
                "NamePseudo",
                "NameProperty",
                "NameTag",
            ],
        );
        add(2100, &["NameBuiltin", "NameBuiltinPseudo"]);
        add(
            2200,
            &[
                "NameVariable",
                "NameVariableAnonymous",
                "NameVariableClass",
                "NameVariableGlobal",
                "NameVariableInstance",
                "NameVariableMagic",
            ],
        );
        add(2300, &["NameFunction", "NameFunctionMagic"]);
        add(3000, &["Literal", "LiteralDate", "LiteralOther"]);
        add(
            3100,
            &[
                "LiteralString",
                "LiteralStringAffix",
                "LiteralStringAtom",
                "LiteralStringBacktick",
                "LiteralStringBoolean",
                "LiteralStringChar",
                "LiteralStringDelimiter",
                "LiteralStringDoc",
                "LiteralStringDouble",
                "LiteralStringEscape",
                "LiteralStringHeredoc",
                "LiteralStringInterpol",
                "LiteralStringName",
                "LiteralStringOther",
                "LiteralStringRegex",
                "LiteralStringSingle",
                "LiteralStringSymbol",
            ],
        );
        add(
            3200,
            &[
                "LiteralNumber",
                "LiteralNumberBin",
                "LiteralNumberFloat",
                "LiteralNumberHex",
                "LiteralNumberInteger",
                "LiteralNumberIntegerLong",
                "LiteralNumberOct",
                "LiteralNumberByte",
            ],
        );
        add(4000, &["Operator", "OperatorWord"]);
        add(5000, &["Punctuation"]);
        add(
            6000,
            &[
                "Comment",
                "CommentHashbang",
                "CommentMultiline",
                "CommentSingle",
                "CommentSpecial",
            ],
        );
        add(6100, &["CommentPreproc", "CommentPreprocFile"]);
        add(
            7000,
            &[
                "Generic",
                "GenericDeleted",
                "GenericEmph",
                "GenericError",
                "GenericHeading",
                "GenericInserted",
                "GenericOutput",
                "GenericPrompt",
                "GenericStrong",
                "GenericSubheading",
                "GenericTraceback",
                "GenericUnderline",
            ],
        );
        add(
            8000,
            &["Text", "TextWhitespace", "TextSymbol", "TextPunctuation"],
        );
        m
    })
}

fn token_type(name: &str) -> TokenType {
    *token_types().get(name).unwrap_or(&ERROR)
}

pub fn sub_category(t: TokenType) -> TokenType {
    t / 100 * 100
}
pub fn category(t: TokenType) -> TokenType {
    t / 1000 * 1000
}

/// Rewrites .NET-style (regexp2) syntax that fancy-regex / regex reject.
pub fn translate(p: &str) -> String {
    let c: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && i + 1 < c.len() {
            let n = c[i + 1];
            match n {
                'u' if i + 5 < c.len()
                    && c[i + 2..].len() >= 4
                    && c[i + 2..i + 6].iter().all(|x| x.is_ascii_hexdigit()) =>
                {
                    out.push_str(&format!(
                        "\\x{{{}}}",
                        c[i + 2..i + 6].iter().collect::<String>()
                    ));
                    i += 6;
                }
                'Z' if !in_class => {
                    out.push_str("(?=\\n?\\z)");
                    i += 2;
                }
                'e' => {
                    out.push_str("\\x1B");
                    i += 2;
                }
                'G' if !in_class => i += 2,
                _ => {
                    out.push(ch);
                    out.push(n);
                    i += 2;
                }
            }
            class_start = false;
            continue;
        }
        if in_class {
            match ch {
                ']' if !class_start => {
                    in_class = false;
                    out.push(']');
                }
                ']' => out.push_str("\\]"),
                '[' => out.push_str("\\["),
                '&' => out.push_str("\\&"),
                '~' => out.push_str("\\~"),
                '-' if i + 1 < c.len() && c[i + 1] == '-' => out.push_str("\\-"),
                '^' if class_start => {
                    out.push('^');
                    i += 1;
                    continue;
                }
                _ => out.push(ch),
            }
            class_start = false;
            i += 1;
            continue;
        }
        match ch {
            '[' => {
                in_class = true;
                class_start = true;
                out.push('[');
            }
            '{' => {
                // Keep valid quantifiers, escape literal braces.
                let rest: String = c[i..].iter().collect();
                if let Some(m) = quantifier_re().find(&rest) {
                    out.push_str(m.as_str());
                    i += m.as_str().chars().count();
                    continue;
                }
                out.push_str("\\{");
            }
            '}' => out.push_str("\\}"),
            '(' if c.get(i + 1) == Some(&'?') && c.get(i + 2) == Some(&'#') => {
                while i < c.len() && c[i] != ')' {
                    i += 1;
                }
            }
            '(' if c.get(i + 1) == Some(&'?') && c.get(i + 2) == Some(&'\'') => {
                out.push_str("(?<");
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    out.push(c[i]);
                    i += 1;
                }
                out.push('>');
            }
            _ => out.push(ch),
        }
        i += 1;
    }
    out
}

fn compile(pattern: &str) -> Option<Regex> {
    RegexBuilder::new(&translate(pattern))
        .backtrack_limit(200_000)
        .build()
        .ok()
}

#[derive(Clone, Debug)]
pub enum Emitter {
    Token(TokenType),
    ByGroups(Vec<Option<Emitter>>),
    Using(String),
    UsingSelf(String),
    UsingByGroup {
        name_group: usize,
        code_group: usize,
        emitters: Vec<Option<Emitter>>,
    },
    UsingRemap {
        lexer: String,
        from: TokenType,
        to: TokenType,
    },
}

#[derive(Clone, Debug)]
pub enum Mutator {
    Push(Vec<String>),
    Pop(usize),
    Include(String),
    Combined(Vec<String>),
    Multi(Vec<Mutator>),
}

pub struct Rule {
    pattern: String,
    emitter: Option<Emitter>,
    mutator: Option<Mutator>,
    re: OnceLock<Option<Regex>>,
}

impl Rule {
    fn new(pattern: &str, emitter: Option<Emitter>, mutator: Option<Mutator>) -> Rule {
        Rule {
            pattern: pattern.to_string(),
            emitter,
            mutator,
            re: OnceLock::new(),
        }
    }
}

#[derive(Default)]
pub struct Config {
    pub name: String,
    pub aliases: Vec<String>,
    pub filenames: Vec<String>,
    pub alias_filenames: Vec<String>,
    pub priority: f32,
    pub case_insensitive: bool,
    pub dot_all: bool,
    pub not_multiline: bool,
    pub ensure_nl: bool,
    analyse_first: bool,
    analyse: Vec<(String, f32)>,
}

enum Source {
    Xml(&'static str),
    Rules(fn() -> Vec<(String, Vec<Rule>)>),
}

pub struct Lexer {
    pub config: Config,
    source: Source,
    analyser: Option<fn(&str) -> f32>,
    rules: OnceLock<HashMap<String, Vec<Arc<Rule>>>>,
    analyse_res: OnceLock<Vec<(Option<Regex>, f32)>>,
}

#[derive(Debug, Default)]
struct XmlElement {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlElement>,
    text: String,
}

impl XmlElement {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
    fn attrs_all(&self, k: &str) -> Vec<String> {
        self.attrs
            .iter()
            .filter(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .collect()
    }
    fn child(&self, n: &str) -> Option<&XmlElement> {
        self.children.iter().find(|c| c.name == n)
    }
}

fn parse_xml(src: &str) -> Option<XmlElement> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(src);
    let mut stack: Vec<XmlElement> = vec![XmlElement::default()];
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => {
                let el = element(&e);
                stack.last_mut()?.children.push(el);
            }
            Event::End(_) => {
                let el = stack.pop()?;
                stack.last_mut()?.children.push(el);
            }
            Event::Text(t) => {
                let s = t.into_inner();
                let s = quick_xml::escape::unescape(&s)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|_| s.to_string());
                stack.last_mut()?.text.push_str(&s);
            }
            Event::GeneralRef(r) => {
                let s = match r.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    _ => {
                        let name = r.into_inner();
                        quick_xml::escape::unescape(&format!("&{name};"))
                            .map(|c| c.to_string())
                            .unwrap_or_default()
                    }
                };
                stack.last_mut()?.text.push_str(&s);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack.pop()?.children.into_iter().next()
}

fn element(e: &quick_xml::events::BytesStart) -> XmlElement {
    let name = e.name().as_ref().to_string();
    let attrs = e
        .attributes()
        .with_checks(false)
        .filter_map(|a| a.ok())
        .map(|a| {
            let k = a.key.as_ref().to_string();
            let raw = a.value.to_string();
            let v = quick_xml::escape::unescape(&raw)
                .map(|c| c.to_string())
                .unwrap_or(raw);
            (k, v)
        })
        .collect();
    XmlElement {
        name,
        attrs,
        children: vec![],
        text: String::new(),
    }
}

fn parse_config(el: &XmlElement) -> Config {
    let mut c = Config::default();
    for ch in &el.children {
        let t = ch.text.trim().to_string();
        match ch.name.as_str() {
            "name" => c.name = t,
            "alias" => c.aliases.push(t),
            "filename" => c.filenames.push(t),
            "alias_filename" => c.alias_filenames.push(t),
            "priority" => c.priority = t.parse().unwrap_or(0.0),
            "case_insensitive" => c.case_insensitive = t == "true",
            "dot_all" => c.dot_all = t == "true",
            "not_multiline" => c.not_multiline = t == "true",
            "ensure_nl" => c.ensure_nl = t == "true",
            "analyse" => {
                c.analyse_first = ch.attr("first") == Some("true");
                for r in &ch.children {
                    if r.name == "regex" {
                        let score = r.attr("score").and_then(|s| s.parse().ok()).unwrap_or(0.0);
                        c.analyse
                            .push((r.attr("pattern").unwrap_or("").to_string(), score));
                    }
                }
            }
            _ => {}
        }
    }
    c
}

fn parse_emitter(el: &XmlElement) -> Option<Emitter> {
    match el.name.as_str() {
        "token" => Some(Emitter::Token(token_type(
            el.attr("type").unwrap_or("Error"),
        ))),
        "bygroups" => Some(Emitter::ByGroups(
            el.children.iter().map(parse_emitter).collect(),
        )),
        "using" => Some(Emitter::Using(el.attr("lexer").unwrap_or("").to_string())),
        "usingself" => Some(Emitter::UsingSelf(
            el.attr("state").unwrap_or("root").to_string(),
        )),
        "usingbygroup" => {
            let num = |n: &str| {
                el.child(n)
                    .and_then(|c| c.text.trim().parse().ok())
                    .unwrap_or(0)
            };
            let emitters = el
                .child("emitters")
                .map(|e| e.children.iter().map(parse_emitter).collect())
                .unwrap_or_default();
            Some(Emitter::UsingByGroup {
                name_group: num("sublexer_name_group"),
                code_group: num("code_group"),
                emitters,
            })
        }
        _ => None,
    }
}

fn parse_mutator(el: &XmlElement) -> Option<Mutator> {
    match el.name.as_str() {
        "push" => Some(Mutator::Push(el.attrs_all("state"))),
        "pop" => Some(Mutator::Pop(
            el.attr("depth").and_then(|d| d.parse().ok()).unwrap_or(1),
        )),
        "include" => Some(Mutator::Include(el.attr("state").unwrap_or("").to_string())),
        "combined" => Some(Mutator::Combined(el.attrs_all("state"))),
        "mutators" => Some(Mutator::Multi(
            el.children.iter().filter_map(parse_mutator).collect(),
        )),
        _ => None,
    }
}

fn parse_rules(el: &XmlElement) -> Vec<(String, Vec<Rule>)> {
    let Some(rules) = el.child("rules") else {
        return vec![];
    };
    rules
        .children
        .iter()
        .filter(|s| s.name == "state")
        .map(|s| {
            let name = s.attr("name").unwrap_or("").to_string();
            let rs = s
                .children
                .iter()
                .filter(|r| r.name == "rule")
                .map(|r| {
                    let mut emitter = None;
                    let mut mutator = None;
                    for ch in &r.children {
                        if let Some(m) = parse_mutator(ch) {
                            mutator = Some(m);
                        } else if let Some(e) = parse_emitter(ch) {
                            emitter = Some(e);
                        }
                    }
                    Rule::new(r.attr("pattern").unwrap_or(""), emitter, mutator)
                })
                .collect();
            (name, rs)
        })
        .collect()
}

impl Lexer {
    fn from_xml(src: &'static str) -> Option<Lexer> {
        // Only the <config> is read eagerly; rules load on first use.
        let start = src.find("<config")?;
        let end = src.find("</config>")? + "</config>".len();
        let el = parse_xml(&src[start..end])?;
        Some(Lexer {
            config: parse_config(&el),
            source: Source::Xml(src),
            analyser: None,
            rules: OnceLock::new(),
            analyse_res: OnceLock::new(),
        })
    }

    fn rules(&self) -> &HashMap<String, Vec<Arc<Rule>>> {
        self.rules.get_or_init(|| {
            let raw = match &self.source {
                Source::Xml(src) => parse_xml(src)
                    .map(|el| parse_rules(&el))
                    .unwrap_or_default(),
                Source::Rules(f) => f(),
            };
            let mut map: HashMap<String, Vec<Arc<Rule>>> = raw
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().map(Arc::new).collect()))
                .collect();
            expand(&mut map);
            map
        })
    }

    fn flags(&self) -> String {
        let mut f = String::new();
        if !self.config.not_multiline {
            f.push('m');
        }
        if self.config.case_insensitive {
            f.push('i');
        }
        if self.config.dot_all {
            f.push('s');
        }
        f
    }

    fn regex<'a>(&self, rule: &'a Rule) -> Option<&'a Regex> {
        rule.re
            .get_or_init(|| {
                let mut p = format!("(?:{})", rule.pattern);
                let f = self.flags();
                if !f.is_empty() {
                    p = format!("(?{f}){p}");
                }
                compile(&p)
            })
            .as_ref()
    }

    pub fn analyse_text(&self, text: &str) -> f32 {
        if let Some(f) = self.analyser {
            return f(text);
        }
        if self.config.analyse.is_empty() {
            return 0.0;
        }
        let res = self.analyse_res.get_or_init(|| {
            self.config
                .analyse
                .iter()
                .map(|(p, s)| {
                    let p = if self.config.case_insensitive {
                        format!("(?i){p}")
                    } else {
                        p.clone()
                    };
                    (compile(&p), *s)
                })
                .collect()
        });
        let mut score = 0.0f32;
        for (re, s) in res {
            let ok = re
                .as_ref()
                .and_then(|r| r.is_match(text).ok())
                .unwrap_or(false);
            if ok && self.config.analyse_first {
                return s.min(1.0);
            }
            if ok {
                score += s;
            }
        }
        score.min(1.0)
    }
}

/// Expands include and combined mutators.
fn expand(map: &mut HashMap<String, Vec<Arc<Rule>>>) {
    let mut guard = 0;
    'restart: loop {
        guard += 1;
        if guard > 10_000 {
            return;
        }
        let states: Vec<String> = map.keys().cloned().collect();
        for state in states {
            let len = map[&state].len();
            for i in 0..len {
                let m = map[&state][i].mutator.clone();
                match m {
                    Some(Mutator::Include(target)) => {
                        let included = map.get(&target).cloned().unwrap_or_default();
                        let rules = map.get_mut(&state).unwrap();
                        rules.splice(i..i + 1, included);
                        continue 'restart;
                    }
                    Some(Mutator::Combined(states)) => {
                        let name = format!("__combined_{}", states.join("__"));
                        if !map.contains_key(&name) {
                            let mut combined = vec![];
                            for s in &states {
                                combined.extend(map.get(s).cloned().unwrap_or_default());
                            }
                            map.insert(name.clone(), combined);
                        }
                        let old = &map[&state][i];
                        let replaced = Rule::new(
                            &old.pattern,
                            old.emitter.clone(),
                            Some(Mutator::Push(vec![name])),
                        );
                        map.get_mut(&state).unwrap()[i] = Arc::new(replaced);
                        continue 'restart;
                    }
                    _ => {}
                }
            }
        }
        return;
    }
}

pub type Token = (TokenType, String);

fn ensure_lf(t: &str) -> String {
    t.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn tokenise(lexer: &Lexer, text: &str, state: &str, nested: bool) -> Vec<Token> {
    let mut text = if nested {
        text.to_string()
    } else {
        ensure_lf(text)
    };
    let mut added = false;
    if !nested && lexer.config.ensure_nl && !text.ends_with('\n') {
        text.push('\n');
        added = true;
    }
    let rules = lexer.rules();
    let end = if added { text.len() - 1 } else { text.len() };
    let mut out: Vec<Token> = vec![];
    let mut stack: Vec<String> = vec![state.to_string()];
    let mut pos = 0usize;
    let mut idle = 0usize;
    while pos < end && !stack.is_empty() {
        let cur = stack.last().unwrap().clone();
        let Some(state_rules) = rules.get(&cur) else {
            break;
        };
        let mut matched: Option<(Arc<Rule>, Vec<String>)> = None;
        for rule in state_rules {
            let Some(re) = lexer.regex(rule) else {
                continue;
            };
            if let Ok(Some(caps)) = re.captures_from_pos(&text, pos)
                && caps.get(0).map(|m| m.start()) == Some(pos)
            {
                let groups = (0..caps.len())
                    .map(|i| {
                        caps.get(i)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                matched = Some((rule.clone(), groups));
                break;
            }
        }
        let Some((rule, groups)) = matched else {
            if text[pos..].starts_with('\n') && cur != state {
                stack = vec![state.to_string()];
                continue;
            }
            let ch = text[pos..].chars().next().unwrap();
            pos += ch.len_utf8();
            out.push((ERROR, ch.to_string()));
            continue;
        };
        let before = stack.clone();
        pos += groups[0].len();
        if let Some(m) = &rule.mutator
            && !mutate(m, &mut stack, &cur)
        {
            break;
        }
        if let Some(e) = &rule.emitter {
            emit(e, &groups, lexer, &mut out);
        }
        // Guard against rules that match nothing and change nothing.
        if groups[0].is_empty() && stack == before {
            idle += 1;
            if idle > 1000 {
                break;
            }
        } else {
            idle = 0;
        }
    }
    if pos < text.len() && stack.is_empty() {
        out.push((ERROR, text[pos..].to_string()));
    }
    out.retain(|(t, v)| *t != IGNORE && !v.is_empty());
    out
}

fn mutate(m: &Mutator, stack: &mut Vec<String>, cur: &str) -> bool {
    match m {
        Mutator::Push(states) if states.is_empty() => stack.push(cur.to_string()),
        Mutator::Push(states) => {
            for s in states {
                if s == "#pop" {
                    stack.pop();
                } else {
                    stack.push(s.clone());
                }
            }
        }
        Mutator::Pop(n) => {
            if stack.is_empty() {
                return false;
            }
            let keep = stack.len().saturating_sub(*n);
            stack.truncate(keep);
        }
        Mutator::Multi(ms) => {
            for m in ms {
                if !mutate(m, stack, cur) {
                    return false;
                }
            }
        }
        Mutator::Include(_) | Mutator::Combined(_) => {}
    }
    true
}

fn emit(e: &Emitter, groups: &[String], lexer: &Lexer, out: &mut Vec<Token>) {
    match e {
        Emitter::Token(t) => out.push((*t, groups[0].clone())),
        Emitter::ByGroups(ems) => {
            if ems.len() != groups.len() - 1 {
                out.push((ERROR, groups[0].clone()));
                return;
            }
            for (i, g) in groups[1..].iter().enumerate() {
                if let Some(em) = &ems[i] {
                    emit(em, std::slice::from_ref(g), lexer, out);
                }
            }
        }
        Emitter::Using(name) => {
            if let Some(l) = get(name) {
                out.extend(tokenise(l, &groups[0], "root", true));
            }
        }
        Emitter::UsingSelf(state) => out.extend(tokenise(lexer, &groups[0], state, true)),
        Emitter::UsingRemap {
            lexer: name,
            from,
            to,
        } => {
            if let Some(l) = get(name) {
                out.extend(
                    tokenise(l, &groups[0], "root", true)
                        .into_iter()
                        .map(|(t, v)| (if t == *from { *to } else { t }, v)),
                );
            }
        }
        Emitter::UsingByGroup {
            name_group,
            code_group,
            emitters,
        } => {
            let sub = groups.get(*name_group).and_then(|n| get(n));
            for (i, g) in groups[1..].iter().enumerate() {
                if let Some(sub) = sub.filter(|_| i + 1 == *code_group) {
                    out.extend(tokenise(sub, &groups[*code_group], "root", false));
                } else if let Some(Some(em)) = emitters.get(i) {
                    emit(em, std::slice::from_ref(g), lexer, out);
                }
            }
        }
    }
}

fn words(prefix: &str, suffix: &str, words: &[&str]) -> String {
    let mut w: Vec<&str> = words.to_vec();
    w.sort_by_key(|b| std::cmp::Reverse(b.len()));
    let quoted: Vec<String> = w.iter().map(|s| regex::escape(s)).collect();
    format!("{prefix}({}){suffix}", quoted.join("|"))
}

fn token(name: &str) -> Option<Emitter> {
    Some(Emitter::Token(token_type(name)))
}

fn bygroups(names: &[Option<Emitter>]) -> Option<Emitter> {
    Some(Emitter::ByGroups(names.to_vec()))
}

fn go_rules() -> Vec<(String, Vec<Rule>)> {
    let r = |p: &str, e: Option<Emitter>| Rule::new(p, e, None);
    let builtins = [
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "float",
        "float32",
        "float64",
        "complex64",
        "complex128",
        "byte",
        "rune",
        "string",
        "bool",
        "error",
        "uintptr",
        "print",
        "println",
        "panic",
        "recover",
        "close",
        "complex",
        "real",
        "imag",
        "len",
        "cap",
        "append",
        "copy",
        "delete",
        "new",
        "make",
        "clear",
        "min",
        "max",
    ];
    let types = [
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "float",
        "float32",
        "float64",
        "complex64",
        "complex128",
        "byte",
        "rune",
        "string",
        "bool",
        "error",
        "uintptr",
        "any",
    ];
    vec![(
        "root".into(),
        vec![
            r(r"\n", token("TextWhitespace")),
            r(r"\s+", token("TextWhitespace")),
            r(r"//[^\s][^\n\r]*", token("CommentPreproc")),
            r(r"//\s+[^\n\r]*", token("CommentSingle")),
            r(r"/(\\\n)?[*](.|\n)*?[*](\\\n)?/", token("CommentMultiline")),
            r(r"(import|package)\b", token("KeywordNamespace")),
            r(
                r"(var|func|struct|map|chan|type|interface|const)\b",
                token("KeywordDeclaration"),
            ),
            r(
                &words(
                    "",
                    r"\b",
                    &[
                        "break",
                        "default",
                        "select",
                        "case",
                        "defer",
                        "go",
                        "else",
                        "goto",
                        "switch",
                        "fallthrough",
                        "if",
                        "range",
                        "continue",
                        "for",
                        "return",
                    ],
                ),
                token("Keyword"),
            ),
            r(r"(true|false|iota|nil)\b", token("KeywordConstant")),
            r(
                &words("", r"\b(\()", &builtins),
                bygroups(&[token("NameBuiltin"), token("Punctuation")]),
            ),
            r(&words("", r"\b", &types), token("KeywordType")),
            r(r"\d+i", token("LiteralNumber")),
            r(r"\d+\.\d*([Ee][-+]\d+)?i", token("LiteralNumber")),
            r(r"\.\d+([Ee][-+]\d+)?i", token("LiteralNumber")),
            r(r"\d+[Ee][-+]\d+i", token("LiteralNumber")),
            r(
                r"\d+(\.\d+[eE][+\-]?\d+|\.\d*|[eE][+\-]?\d+)",
                token("LiteralNumberFloat"),
            ),
            r(r"\.\d+([eE][+\-]?\d+)?", token("LiteralNumberFloat")),
            r(r"0[0-7]+", token("LiteralNumberOct")),
            r(r"0[xX][0-9a-fA-F_]+", token("LiteralNumberHex")),
            r(r"0b[01_]+", token("LiteralNumberBin")),
            r(r"(0|[1-9][0-9_]*)", token("LiteralNumberInteger")),
            r(
                r#"'(\\['"\\abfnrtv]|\\x[0-9a-fA-F]{2}|\\[0-7]{1,3}|\\u[0-9a-fA-F]{4}|\\U[0-9a-fA-F]{8}|[^\\])'"#,
                token("LiteralStringChar"),
            ),
            r(
                "(`)([^`]*)(`)",
                bygroups(&[
                    token("LiteralString"),
                    Some(Emitter::UsingRemap {
                        lexer: "Go Text Template".into(),
                        from: OTHER,
                        to: token_type("LiteralString"),
                    }),
                    token("LiteralString"),
                ]),
            ),
            r(r#""(\\\\|\\"|[^"])*""#, token("LiteralString")),
            r(
                r"(<<=|>>=|<<|>>|<=|>=|&\^=|&\^|\+=|-=|\*=|/=|%=|&=|\|=|&&|\|\||<-|\+\+|--|==|!=|:=|\.\.\.|[+\-*/%&])",
                token("Operator"),
            ),
            r(
                r"([a-zA-Z_]\w*)(\s*)(\()",
                bygroups(&[
                    token("NameFunction"),
                    Some(Emitter::UsingSelf("root".into())),
                    token("Punctuation"),
                ]),
            ),
            r(r"[|^<>=!()\[\]{}.,;:~]", token("Punctuation")),
            r(r"[^\W\d]\w*", token("NameOther")),
        ],
    )]
}

fn markdown_rules() -> Vec<(String, Vec<Rule>)> {
    let r = |p: &str, e: Option<Emitter>| Rule::new(p, e, None);
    let inline_self = Some(Emitter::UsingSelf("inline".into()));
    vec![
        (
            "root".into(),
            vec![
                r(r"^(#[^#].+\n)", bygroups(&[token("GenericHeading")])),
                r(r"^(#{2,6}.+\n)", bygroups(&[token("GenericSubheading")])),
                r(
                    r"^(\s*)([*-] )(\[[ xX]\])( .+\n)",
                    bygroups(&[
                        token("Text"),
                        token("Keyword"),
                        token("Keyword"),
                        inline_self.clone(),
                    ]),
                ),
                r(
                    r"^(\s*)([*-])(\s)(.+\n)",
                    bygroups(&[
                        token("Text"),
                        token("Keyword"),
                        token("Text"),
                        inline_self.clone(),
                    ]),
                ),
                r(
                    r"^(\s*)([0-9]+\.)( .+\n)",
                    bygroups(&[token("Text"), token("Keyword"), inline_self.clone()]),
                ),
                r(
                    r"^(\s*>\s)(.+\n)",
                    bygroups(&[token("Keyword"), token("GenericEmph")]),
                ),
                r(
                    "^(```\\n)([\\w\\W]*?)(^```$)",
                    bygroups(&[
                        token("LiteralString"),
                        token("Text"),
                        token("LiteralString"),
                    ]),
                ),
                r(
                    "^(```)(\\w+)(\\n)([\\w\\W]*?)(^```$)",
                    Some(Emitter::UsingByGroup {
                        name_group: 2,
                        code_group: 4,
                        emitters: vec![
                            token("LiteralString"),
                            token("LiteralString"),
                            token("LiteralString"),
                            token("Text"),
                            token("LiteralString"),
                        ],
                    }),
                ),
                Rule::new("", None, Some(Mutator::Include("inline".into()))),
            ],
        ),
        (
            "inline".into(),
            vec![
                r(r"\\.", token("Text")),
                r(
                    r"(\s)(\*|_)((?:(?!\2).)*)(\2)((?=\W|\n))",
                    bygroups(&[
                        token("Text"),
                        token("GenericEmph"),
                        token("GenericEmph"),
                        token("GenericEmph"),
                        token("Text"),
                    ]),
                ),
                r(
                    r"(\s)((\*\*|__).*?)\3((?=\W|\n))",
                    bygroups(&[
                        token("Text"),
                        token("GenericStrong"),
                        token("GenericStrong"),
                        token("Text"),
                    ]),
                ),
                r(
                    r"(\s)(~~[^~]+~~)((?=\W|\n))",
                    bygroups(&[token("Text"), token("GenericDeleted"), token("Text")]),
                ),
                r("`[^`]+`", token("LiteralStringBacktick")),
                r(r"[@#][\w/:]+", token("NameEntity")),
                r(
                    r"(!?\[)([^]]+)(\])(\()([^)]+)(\))",
                    bygroups(&[
                        token("Text"),
                        token("NameTag"),
                        token("Text"),
                        token("Text"),
                        token("NameAttribute"),
                        token("Text"),
                    ]),
                ),
                r(r".|\n", token("Text")),
            ],
        ),
    ]
}

fn plaintext_rules() -> Vec<(String, Vec<Rule>)> {
    vec![(
        "root".into(),
        vec![
            Rule::new(".+", token("Text"), None),
            Rule::new(r"\n", token("Text"), None),
        ],
    )]
}

fn go_analyser(text: &str) -> f32 {
    if text.contains("fmt.") && text.contains("package ") {
        0.5
    } else if text.contains("package ") {
        0.1
    } else {
        0.0
    }
}

fn mysql_analyser(text: &str) -> f32 {
    static BT: OnceLock<regex::Regex> = OnceLock::new();
    static BR: OnceLock<regex::Regex> = OnceLock::new();
    let bt = BT
        .get_or_init(|| regex::Regex::new(r"`[a-zA-Z_]\w*`").unwrap())
        .find_iter(text)
        .count();
    let br = BR
        .get_or_init(|| regex::Regex::new(r"\[[a-zA-Z_]\w*\]").unwrap())
        .find_iter(text)
        .count();
    if bt + br >= 1 && bt >= 2 * br {
        0.5
    } else if bt > br {
        0.2
    } else if bt > 0 {
        0.1
    } else {
        0.0
    }
}

fn dns_analyser(text: &str) -> f32 {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    if RE
        .get_or_init(|| regex::Regex::new(r"(?m)^@\s+IN\s+SOA\s+").unwrap())
        .is_match(text)
    {
        1.0
    } else {
        0.0
    }
}

fn zed_analyser(text: &str) -> f32 {
    let (d, r, p) = (
        text.contains("definition "),
        text.contains("relation "),
        text.contains("permission "),
    );
    if d && r && p {
        0.9
    } else if d || r {
        0.5
    } else if p {
        0.25
    } else {
        0.0
    }
}

fn go_lexer(
    name: &str,
    aliases: &[&str],
    filenames: &[&str],
    rules: fn() -> Vec<(String, Vec<Rule>)>,
) -> Lexer {
    Lexer {
        config: Config {
            name: name.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            filenames: filenames.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        },
        source: Source::Rules(rules),
        analyser: None,
        rules: OnceLock::new(),
        analyse_res: OnceLock::new(),
    }
}

pub struct Registry {
    pub lexers: Vec<Lexer>,
    pub fallback: Lexer,
}

pub fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| {
        let mut lexers: Vec<Lexer> = vec![];
        let mut add = |l: Lexer| {
            if let Some(i) = lexers.iter().position(|x| x.config.name == l.config.name) {
                lexers[i] = l;
            } else {
                lexers.push(l);
            }
        };
        for (_, src) in EMBEDDED {
            if let Some(l) = Lexer::from_xml(src) {
                add(l);
            }
        }
        let mut go = go_lexer("Go", &["go", "golang"], &["*.go"], go_rules);
        go.analyser = Some(go_analyser);
        add(go);
        if let Some((_, src)) = EMBEDDED.iter().find(|(n, _)| *n == "go_template.xml")
            && let Some(mut l) = Lexer::from_xml(src)
        {
            l.config.name = "Go Text Template".into();
            l.config.aliases = vec!["go-text-template".into()];
            l.config.filenames.clear();
            l.config.analyse.clear();
            add(l);
        }
        add(go_lexer(
            "markdown",
            &["md", "mkd"],
            &["*.md", "*.mkd", "*.markdown"],
            markdown_rules,
        ));
        // Chroma attaches these analysers to XML lexers in its Go code.
        for (name, f) in [
            ("mysql", mysql_analyser as fn(&str) -> f32),
            ("dns", dns_analyser),
            ("zed", zed_analyser),
        ] {
            if let Some(l) = lexers.iter_mut().find(|l| {
                l.config.name.to_lowercase() == name || l.config.aliases.iter().any(|a| a == name)
            }) {
                l.analyser = Some(f);
            }
        }
        let mut fallback = go_lexer("fallback", &[], &["*"], plaintext_rules);
        fallback.config.priority = -1.0;
        Registry { lexers, fallback }
    })
}

fn glob_match(glob: &str, name: &str) -> bool {
    fn rec(g: &[char], n: &[char]) -> bool {
        match g.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| rec(&g[1..], &n[i..])),
            Some('?') => !n.is_empty() && rec(&g[1..], &n[1..]),
            Some('[') => {
                let Some(close) = g.iter().position(|c| *c == ']') else {
                    return false;
                };
                let Some(&c) = n.first() else { return false };
                let class = &g[1..close];
                let (neg, class) = match class.first() {
                    Some('^') | Some('!') => (true, &class[1..]),
                    _ => (false, class),
                };
                let mut hit = false;
                let mut i = 0;
                while i < class.len() {
                    if i + 2 < class.len() && class[i + 1] == '-' {
                        hit |= class[i] <= c && c <= class[i + 2];
                        i += 3;
                    } else {
                        hit |= class[i] == c;
                        i += 1;
                    }
                }
                hit != neg && rec(&g[close + 1..], &n[1..])
            }
            Some('\\') if g.len() > 1 => n.first() == Some(&g[1]) && rec(&g[2..], &n[1..]),
            Some(c) => n.first() == Some(c) && rec(&g[1..], &n[1..]),
        }
    }
    rec(
        &glob.chars().collect::<Vec<_>>(),
        &name.chars().collect::<Vec<_>>(),
    )
}

const IGNORED_SUFFIXES: [&str; 13] = [
    "~",
    ".bak",
    ".old",
    ".orig",
    ".dpkg-dist",
    ".dpkg-old",
    ".ucf-dist",
    ".ucf-new",
    ".ucf-old",
    ".rpmnew",
    ".rpmorig",
    ".rpmsave",
    ".in",
];

fn priority(l: &Lexer) -> f32 {
    if l.config.priority == 0.0 {
        1.0
    } else {
        l.config.priority
    }
}

fn match_filename(filename: &str) -> Option<&'static Lexer> {
    let reg = registry();
    let base = filename.rsplit('/').next().unwrap_or(filename);
    for alias in [false, true] {
        let mut matched: Vec<&Lexer> = vec![];
        for l in &reg.lexers {
            let globs = if alias {
                &l.config.alias_filenames
            } else {
                &l.config.filenames
            };
            for g in globs {
                if glob_match(g, base)
                    || IGNORED_SUFFIXES
                        .iter()
                        .any(|s| glob_match(&format!("{g}{s}"), base))
                {
                    matched.push(l);
                }
            }
        }
        if !matched.is_empty() {
            matched.sort_by(|a, b| {
                priority(b)
                    .partial_cmp(&priority(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            return Some(matched[0]);
        }
    }
    None
}

pub fn get(name: &str) -> Option<&'static Lexer> {
    let reg = registry();
    let lower = name.to_lowercase();
    for n in [name, lower.as_str()] {
        if let Some(l) = reg
            .lexers
            .iter()
            .find(|l| l.config.name == n || l.config.name.to_lowercase() == n)
        {
            return Some(l);
        }
        if let Some(l) = reg.lexers.iter().find(|l| {
            l.config
                .aliases
                .iter()
                .any(|a| a == n || a.to_lowercase() == n)
        }) {
            return Some(l);
        }
    }
    if name.is_empty() {
        return None;
    }
    let mut candidates: Vec<&Lexer> = vec![];
    if let Some(l) = match_filename(&format!("filename.{name}")) {
        candidates.push(l);
    }
    if let Some(l) = match_filename(name) {
        candidates.push(l);
    }
    candidates.sort_by(|a, b| {
        priority(b)
            .partial_cmp(&priority(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.first().copied()
}

pub fn analyse(text: &str) -> Option<&'static Lexer> {
    let mut picked = None;
    let mut highest = 0.0f32;
    for l in &registry().lexers {
        let w = l.analyse_text(text);
        if w > highest {
            picked = Some(l);
            highest = w;
        }
    }
    picked
}

/// Chroma's quick.Highlight lexer choice: by name, then by content, then plain text.
pub fn tokenise_code(code: &str, lang: &str) -> Vec<Token> {
    let reg = registry();
    let lexer = get(lang).or_else(|| analyse(code)).unwrap_or(&reg.fallback);
    // Very large blocks are rendered as plain text to bound the regex work.
    if code.len() > 64 * 1024 {
        return tokenise(&reg.fallback, code, "root", false);
    }
    tokenise(lexer, code, "root", false)
}

/// Matches a regex quantifier such as `{2}` or `{2,5}` at the start of a string.
fn quantifier_re() -> &'static regex::Regex {
    static Q: OnceLock<regex::Regex> = OnceLock::new();
    Q.get_or_init(|| regex::Regex::new(r"^\{\d+(,\d*)?\}").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_lexers_load() {
        let reg = registry();
        assert!(reg.lexers.len() > 250);
        assert!(get("js").is_some());
        assert!(get("rust").is_some());
        assert!(get("sh").is_some());
        assert_eq!(get("go").map(|l| l.config.name.as_str()), Some("Go"));
    }

    #[test]
    fn rules_compile_for_common_lexers() {
        for name in [
            "javascript",
            "typescript",
            "bash",
            "python",
            "rust",
            "json",
            "yaml",
            "diff",
            "html",
            "css",
            "toml",
            "go",
            "markdown",
        ] {
            let l = get(name).unwrap();
            let mut bad = vec![];
            for (state, rules) in l.rules() {
                for r in rules {
                    if !r.pattern.is_empty() && l.regex(r).is_none() {
                        bad.push(format!("{state}: {}", r.pattern));
                    }
                }
            }
            assert!(bad.is_empty(), "{name}: {bad:?}");
        }
    }

    #[test]
    #[ignore]
    fn report_uncompilable_rules() {
        let mut total = 0;
        let mut bad = 0;
        for l in &registry().lexers {
            for (state, rules) in l.rules() {
                for r in rules {
                    if r.pattern.is_empty() {
                        continue;
                    }
                    total += 1;
                    if l.regex(r).is_none() {
                        bad += 1;
                        println!("BAD|{}|{state}|{}", l.config.name, r.pattern);
                    }
                }
            }
        }
        println!("TOTAL|{total}|{bad}");
    }

    #[test]
    fn tokenises_go() {
        let toks = tokenise_code("func main() {}\n", "go");
        assert_eq!(toks[0], (token_type("KeywordDeclaration"), "func".into()));
    }

    #[test]
    fn globs() {
        assert!(glob_match("*.go", "filename.go"));
        assert!(glob_match("*.[ch]", "a.h"));
        assert!(!glob_match("*.go", "a.rs"));
    }
}
