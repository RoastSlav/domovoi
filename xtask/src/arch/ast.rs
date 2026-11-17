//! Syntax-aware boundary checks (EXEC-1 to EXEC-4, ADR 0023).
//!
//! Every file is parsed with `syn` and the operations, type names and accessors are counted
//! where the syntax tree writes them, so whitespace, line breaks, qualified paths, renamed
//! imports and comments cannot change the result. Macro bodies cannot be parsed as expressions
//! in general, so their token streams are scanned for the same names and any hit counts: a
//! macro cannot hide a boundary operation. A file that cannot be parsed is a violation, never
//! a pass.
//!
//! This is the one implementation of these rules: `cargo xtask arch-check` and the engine and
//! hub gate tests all call [`exec_violations`] and [`cfg_test_closure`].

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    ExprField, ExprMethodCall, ExprPath, ExprStruct, ItemImpl, ItemMod, Macro, PatStruct, Path,
    Type,
};

use super::Violation;
use super::alias::aliases;
use super::reach::{MapSource, Roots, explore};
use super::rules::{
    BOUNDARY_CARDS, BOUNDARY_PAIRS, BoundaryOp, EXEC_LINK_CRATES, EXEC_OPERATION_CRATES,
    LINK_ACCESSOR_PATHS, LINK_ACCESSORS, LINK_NAMES, LINK_NAMES_ALLOWED_FILES, RAW_OPS,
};
use super::wiring::{Target, edges_of, is_cfg_test};

/// Check the counted boundary operations and the link names over production files.
///
/// `files` holds `(workspace-relative path, contents)` pairs; test-only files must already be
/// filtered out by the caller (see [`cfg_test_closure`]). A file outside `crates/` was loaded
/// by `#[path]` or `include!` from one inside it; it is checked against every crate's rules,
/// because which crate loaded it is not known from the path. The check is default-deny: a counted
/// operation in a file no card lists is a violation, a listed file must hold exactly its
/// card's count, and a card whose file is missing from a crate that is present is a violation
/// because the boundary moved without the card.
pub fn exec_violations(files: &[(String, String)]) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut facts: Vec<(&str, FileFacts)> = Vec::new();
    for (rel, text) in files {
        match file_facts(rel, text) {
            Ok(file) => facts.push((rel.as_str(), file)),
            Err(violation) => out.push(violation),
        }
    }

    for card in BOUNDARY_CARDS {
        for (rel, file) in &facts {
            if !in_crates(rel, EXEC_OPERATION_CRATES) {
                continue;
            }
            let (found, line) = (file.count(&card.op), file.line(&card.op));
            match card.sites.iter().find(|site| site.file == *rel) {
                Some(site) if site.count == found => {}
                Some(site) => out.push(Violation {
                    rule: card.id,
                    location: at(rel, line),
                    message: format!(
                        "{} is called {found} time(s) here; the boundary card allows {}.",
                        describe(&card.op),
                        site.count
                    ),
                }),
                None if found > 0 => out.push(Violation {
                    rule: card.id,
                    location: at(rel, line),
                    message: format!(
                        "{} may not appear here. {}",
                        describe(&card.op),
                        card.message
                    ),
                }),
                None => {}
            }
        }
        for site in card.sites {
            if files.iter().any(|(rel, _)| rel == site.file) {
                continue;
            }
            let prefix = crate_prefix(site.file);
            if files.iter().any(|(rel, _)| rel.starts_with(&prefix)) {
                out.push(Violation {
                    rule: card.id,
                    location: site.file.to_string(),
                    message: format!(
                        "the boundary card expects {} call(s) of {} in this file, but it was not scanned; the boundary moved or the file was removed, so update the card deliberately",
                        site.count,
                        describe(&card.op)
                    ),
                });
            }
        }
    }

    for (rel, file) in &facts {
        if !in_crates(rel, EXEC_LINK_CRATES) {
            continue;
        }
        let summary = file.link_summary();
        if summary.is_empty() || LINK_NAMES_ALLOWED_FILES.contains(rel) {
            continue;
        }
        out.push(Violation {
            rule: "EXEC-4",
            location: at(rel, file.link_line()),
            message: format!(
                "a transport reads host state through AgentAdmin, never by naming or holding the raw link ({summary}); act through the capability objects (ADR 0023)"
            ),
        });
    }

    out
}

/// The files only `#[cfg(test)]` module wiring reaches, for callers that have no Cargo
/// metadata (the engine and hub gate tests).
///
/// The files no other given file loads are the production roots; the wiring is followed from
/// them, and a file is returned only when test-gated declarations reach it and no ordinary
/// declaration does. Names decide nothing.
pub fn cfg_test_closure(files: &[(String, String)]) -> BTreeSet<String> {
    let map: BTreeMap<String, String> = files.iter().cloned().collect();
    let mut loaded: BTreeSet<String> = BTreeSet::new();
    for (rel, text) in &map {
        for edge in edges_of(rel, text, false).unwrap_or_default() {
            match edge.target {
                Target::Module(candidates) => {
                    loaded.extend(candidates.into_iter().filter(|c| map.contains_key(c)));
                }
                Target::Include(Some(path)) if map.contains_key(&path) => {
                    loaded.insert(path);
                }
                _ => {}
            }
        }
    }
    let roots = Roots {
        production: map
            .keys()
            .filter(|rel| !loaded.contains(*rel))
            .cloned()
            .collect(),
        test: BTreeSet::new(),
    };
    explore(&mut MapSource(&map), &roots).test_only()
}

fn in_crates(rel: &str, crates: &[&str]) -> bool {
    !rel.starts_with("crates/") || crates.iter().any(|prefix| rel.starts_with(prefix))
}

fn crate_prefix(file: &str) -> String {
    let mut parts = file.split('/');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    format!("{first}/{second}/")
}

fn describe(op: &BoundaryOp) -> String {
    match op {
        BoundaryOp::Call(name) => format!("`{name}`"),
        BoundaryOp::Pair(key) => {
            format!("`{key}` (as a path, not a struct expression or pattern)")
        }
        BoundaryOp::Construct(key) => format!("a `{key} {{ .. }}` struct expression"),
        BoundaryOp::Pattern(key) => format!("a `{key} {{ .. }}` struct pattern"),
    }
}

fn at(rel: &str, line: usize) -> String {
    if line == 0 {
        rel.to_string()
    } else {
        format!("{rel}:{line}")
    }
}

fn static_of(names: &[&'static str], name: &str) -> Option<&'static str> {
    names.iter().find(|known| **known == name).copied()
}

/// What one file's syntax tree contains, by rule label, with the line of each occurrence.
#[derive(Default)]
struct FileFacts {
    ops: BTreeMap<&'static str, Vec<usize>>,
    pairs: BTreeMap<&'static str, Vec<usize>>,
    constructs: BTreeMap<&'static str, Vec<usize>>,
    patterns: BTreeMap<&'static str, Vec<usize>>,
    names: BTreeMap<&'static str, Vec<usize>>,
    accessors: BTreeMap<&'static str, Vec<usize>>,
}

impl FileFacts {
    fn occurrences(&self, op: &BoundaryOp) -> Option<&Vec<usize>> {
        match op {
            BoundaryOp::Call(name) => self.ops.get(name),
            BoundaryOp::Pair(key) => self.pairs.get(key),
            BoundaryOp::Construct(key) => self.constructs.get(key),
            BoundaryOp::Pattern(key) => self.patterns.get(key),
        }
    }

    fn count(&self, op: &BoundaryOp) -> usize {
        self.occurrences(op).map_or(0, Vec::len)
    }

    fn line(&self, op: &BoundaryOp) -> usize {
        self.occurrences(op)
            .and_then(|lines| lines.first())
            .copied()
            .unwrap_or(0)
    }

    fn link_summary(&self) -> String {
        let mut parts = Vec::new();
        for (name, lines) in &self.names {
            parts.push(format!("`{name}` x{}", lines.len()));
        }
        for (name, lines) in &self.accessors {
            parts.push(format!("`.{name}` x{}", lines.len()));
        }
        parts.join(", ")
    }

    fn link_line(&self) -> usize {
        self.names
            .values()
            .chain(self.accessors.values())
            .filter_map(|lines| lines.first())
            .min()
            .copied()
            .unwrap_or(0)
    }

    /// Record a path written anywhere except as the path of a struct expression or pattern.
    fn record(&mut self, segments: &[String], line: usize) {
        self.record_names(segments, line);
        for &key in BOUNDARY_PAIRS {
            if ends_with_pair(segments, key) {
                self.pairs.entry(key).or_default().push(line);
            }
        }
    }

    /// The operation, link-name and accessor-path facts of a path.
    fn record_names(&mut self, segments: &[String], line: usize) {
        if let Some(last) = segments.last() {
            if let Some(name) = static_of(RAW_OPS, last) {
                self.ops.entry(name).or_default().push(line);
            }
            if let Some(name) = static_of(LINK_NAMES, last) {
                self.names.entry(name).or_default().push(line);
            }
        }
        for &(ty, accessor) in LINK_ACCESSOR_PATHS {
            if segments.len() >= 2
                && segments[segments.len() - 2] == ty
                && segments[segments.len() - 1] == accessor
                && let Some(name) = static_of(LINK_ACCESSORS, accessor)
            {
                self.accessors.entry(name).or_default().push(line);
            }
        }
    }

    /// Record the path of a struct expression (`construct`) or struct pattern, once.
    fn record_struct(&mut self, segments: &[String], line: usize, construct: bool) {
        self.record_names(segments, line);
        for &key in BOUNDARY_PAIRS {
            if ends_with_pair(segments, key) {
                let bucket = if construct {
                    &mut self.constructs
                } else {
                    &mut self.patterns
                };
                bucket.entry(key).or_default().push(line);
            }
        }
    }
}

/// Whether the last two segments are the `Type::assoc` pair `key` names.
fn ends_with_pair(segments: &[String], key: &str) -> bool {
    let parts: Vec<&str> = key.split("::").collect();
    parts.len() == 2
        && segments.len() >= 2
        && segments[segments.len() - 2] == parts[0]
        && segments[segments.len() - 1] == parts[1]
}

/// Parse one file and collect its facts. A file that cannot be parsed is a violation: the
/// boundary cannot be shown to hold, so it is refused.
fn file_facts(rel: &str, text: &str) -> Result<FileFacts, Violation> {
    match syn::parse_file(text) {
        Ok(file) => {
            let aliases = aliases(&file);
            let mut counter = Counter {
                aliases: &aliases,
                facts: FileFacts::default(),
                self_types: Vec::new(),
            };
            counter.visit_file(&file);
            Ok(counter.facts)
        }
        Err(error) => {
            match syn::parse_str::<syn::ItemFn>(&format!("fn __probe() {{\n{text}\n}}")) {
                Ok(item) => {
                    let aliases = BTreeMap::new();
                    let mut counter = Counter {
                        aliases: &aliases,
                        facts: FileFacts::default(),
                        self_types: Vec::new(),
                    };
                    counter.visit_item_fn(&item);
                    Ok(counter.facts)
                }
                Err(_) => Err(Violation {
                    rule: "EXEC",
                    location: rel.to_string(),
                    message: format!(
                        "cannot parse this file, so the execution boundary cannot be checked (fail-closed): {error}"
                    ),
                }),
            }
        }
    }
}

/// Counts operations, constructions, link names and accessors in one syntax tree.
struct Counter<'a> {
    aliases: &'a BTreeMap<String, Vec<String>>,
    facts: FileFacts,
    /// The self type of each enclosing `impl`, innermost last, so `Self::link` resolves.
    self_types: Vec<Option<Vec<String>>>,
}

impl<'a, 'ast> Visit<'ast> for Counter<'a> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let own = match (&node.trait_, &*node.self_ty) {
            (None, Type::Path(path)) if path.qself.is_none() => Some(self.expanded(&path.path)),
            _ => None,
        };
        self.self_types.push(own);
        syn::visit::visit_item_impl(self, node);
        self.self_types.pop();
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        // `<AgentServer>::link`: the type is in the qualified self, the accessor is the path.
        if let Some(qself) = &node.qself
            && let Type::Path(ty) = &*qself.ty
            && let Some(accessor) = node.path.segments.last()
        {
            let mut segments = self.expanded(&ty.path);
            segments.push(accessor.ident.to_string());
            let line = accessor.ident.span().start().line;
            self.facts.record_names(&segments, line);
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_struct(&mut self, node: &'ast ExprStruct) {
        let segments = self.expanded(&node.path);
        self.facts
            .record_struct(&segments, node.path.span().start().line, true);
        for field in &node.fields {
            self.visit_field_value(field);
        }
        if let Some(rest) = &node.rest {
            self.visit_expr(rest);
        }
    }

    fn visit_pat_struct(&mut self, node: &'ast PatStruct) {
        let segments = self.expanded(&node.path);
        self.facts
            .record_struct(&segments, node.path.span().start().line, false);
        for field in &node.fields {
            self.visit_field_pat(field);
        }
    }

    fn visit_path(&mut self, node: &'ast Path) {
        let segments = self.expanded(node);
        let line = node.span().start().line;
        self.facts.record(&segments, line);
        syn::visit::visit_path(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let name = node.method.to_string();
        let line = node.method.span().start().line;
        if let Some(op) = static_of(RAW_OPS, &name) {
            self.facts.ops.entry(op).or_default().push(line);
        }
        if let Some(accessor) = static_of(LINK_ACCESSORS, &name) {
            self.facts.accessors.entry(accessor).or_default().push(line);
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast ExprField) {
        if let syn::Member::Named(ident) = &node.member {
            let name = ident.to_string();
            if let Some(accessor) = static_of(LINK_ACCESSORS, &name) {
                self.facts
                    .accessors
                    .entry(accessor)
                    .or_default()
                    .push(ident.span().start().line);
            }
        }
        syn::visit::visit_expr_field(self, node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        scan_tokens(&mut self.facts, self.aliases, &node.tokens);
        syn::visit::visit_macro(self, node);
    }
}

impl Counter<'_> {
    /// A path's segments, with a `use` alias at the front expanded to the imported path.
    fn expanded(&self, path: &Path) -> Vec<String> {
        let mut segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if segments.first().is_some_and(|first| first == "Self")
            && let Some(Some(own)) = self.self_types.last()
        {
            let mut expanded = own.clone();
            expanded.extend(segments.drain(1..));
            return expanded;
        }
        if let Some(first) = segments.first()
            && let Some(full) = self.aliases.get(first)
        {
            let mut expanded = full.clone();
            expanded.extend(segments.drain(1..));
            segments = expanded;
        }
        segments
    }
}

/// Scan a macro body for the same names. Every identifier counts, and an identifier pair joined
/// by `::` counts as a path, so `run_tool` and `AuthorizedExecution::of` cannot hide in a macro.
fn scan_tokens(
    facts: &mut FileFacts,
    aliases: &BTreeMap<String, Vec<String>>,
    tokens: &TokenStream,
) {
    let flat: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (index, token) in flat.iter().enumerate() {
        if let TokenTree::Group(group) = token {
            scan_tokens(facts, aliases, &group.stream());
        }
        let TokenTree::Ident(ident) = token else {
            continue;
        };
        let name = ident.to_string();
        let line = ident.span().start().line;
        if let Some(accessor) = static_of(LINK_ACCESSORS, &name) {
            facts.accessors.entry(accessor).or_default().push(line);
        }
        let segments = match aliases.get(&name) {
            Some(full) => full.clone(),
            None => vec![name],
        };
        facts.record(&segments, line);
        let joined = matches!(
            (flat.get(index + 1), flat.get(index + 2), flat.get(index + 3)),
            (
                Some(TokenTree::Punct(first)),
                Some(TokenTree::Punct(second)),
                Some(TokenTree::Ident(_))
            ) if first.as_char() == ':' && second.as_char() == ':'
        );
        if joined && let Some(TokenTree::Ident(next)) = flat.get(index + 3) {
            let mut pair = segments;
            pair.push(next.to_string());
            facts.record(&pair, next.span().start().line);
        }
    }
}

#[cfg(test)]
#[path = "ast_tests.rs"]
mod tests;
