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
    Attribute, Expr, ExprField, ExprMethodCall, ItemMod, ItemUse, Macro, Meta, Path, Token, UseTree,
};

use super::Violation;
use super::rules::{
    BOUNDARY_CARDS, BOUNDARY_PAIRS, BoundaryOp, EXEC_LINK_CRATES, EXEC_OPERATION_CRATES,
    LINK_ACCESSORS, LINK_NAMES, LINK_NAMES_ALLOWED_FILES, RAW_OPS,
};

/// Check the counted boundary operations and the link names over production files.
///
/// `files` holds `(workspace-relative path, contents)` pairs; test-only files must already be
/// filtered out by the caller (see [`test_only_files`]). The check is default-deny: a counted
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
            let (found, line) = match &card.op {
                BoundaryOp::Call(name) => (file.op_count(name), file.op_line(name)),
                BoundaryOp::Pair(key) => (file.pair_count(key), file.pair_line(key)),
            };
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
                        "{} may not be called here. {}",
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

/// One external module declaration: where the file may live, and whether only tests compile it.
pub struct ModRef {
    /// Candidate workspace-relative paths: one for `#[path]`, otherwise `name.rs` then
    /// `name/mod.rs`. The caller keeps the candidate that exists.
    pub candidates: Vec<String>,
    /// True when the declaration or an enclosing module is gated by `#[cfg(test)]`.
    pub cfg_test: bool,
}

/// External `mod` declarations in one file, resolved from the file's own location.
///
/// `#[path]` is taken relative to the declaring file's directory and plain modules relative to
/// the module's directory (`mod.rs`/`lib.rs`/`main.rs` use their own directory, `foo.rs` uses
/// `foo/`), which is how `rustc` resolves them. Inline modules are followed, and `cfg(test)`
/// gating is inherited.
pub fn mod_refs(rel: &str, text: &str) -> Vec<ModRef> {
    let Ok(file) = syn::parse_file(text) else {
        return Vec::new();
    };
    let mut walker = ModuleWalker {
        plain: module_dir(rel),
        attr_base: parent_dir(rel),
        tainted: false,
        refs: Vec::new(),
    };
    walker.visit_file(&file);
    walker.refs
}

/// The files compiled only for tests, from the given test-target roots and `#[cfg(test)]`
/// module wiring. Files are workspace-relative with `/` separators.
///
/// A file counts as test-only when a test target owns it, when a production module declares
/// it under `#[cfg(test)]`, or when a test-only module reaches it. Names decide nothing.
pub fn test_only_files(files: &[(String, String)], roots: &BTreeSet<String>) -> BTreeSet<String> {
    let existing: BTreeSet<&str> = files.iter().map(|(rel, _)| rel.as_str()).collect();
    let refs: Vec<(&str, Vec<ModRef>)> = files
        .iter()
        .map(|(rel, text)| (rel.as_str(), mod_refs(rel, text)))
        .collect();

    let mut test: BTreeSet<String> = roots
        .iter()
        .filter(|root| existing.contains(root.as_str()))
        .cloned()
        .collect();
    for (_, modules) in &refs {
        for module in modules.iter().filter(|module| module.cfg_test) {
            for candidate in resolve(module, &existing) {
                test.insert(candidate);
            }
        }
    }
    loop {
        let mut grew = false;
        let reached: Vec<String> = test.iter().cloned().collect();
        for rel in reached {
            let Some((_, modules)) = refs.iter().find(|(file, _)| *file == rel) else {
                continue;
            };
            for module in modules {
                for candidate in resolve(module, &existing) {
                    grew |= test.insert(candidate);
                }
            }
        }
        if !grew {
            return test;
        }
    }
}

/// The test-only files reached from `#[cfg(test)]` module wiring alone, without Cargo
/// metadata. Gate tests call this to keep their crate's test modules out of the production
/// set, and `crate::arch::scan` adds the metadata's test-target roots on top.
pub fn cfg_test_closure(files: &[(String, String)]) -> BTreeSet<String> {
    test_only_files(files, &BTreeSet::new())
}

fn in_crates(rel: &str, crates: &[&str]) -> bool {
    crates.iter().any(|prefix| rel.starts_with(prefix))
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
        BoundaryOp::Pair(key) => format!("`{key}`"),
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
    names: BTreeMap<&'static str, Vec<usize>>,
    accessors: BTreeMap<&'static str, Vec<usize>>,
}

impl FileFacts {
    fn op_count(&self, name: &str) -> usize {
        self.ops.get(name).map_or(0, Vec::len)
    }

    fn op_line(&self, name: &str) -> usize {
        self.ops
            .get(name)
            .and_then(|lines| lines.first())
            .copied()
            .unwrap_or(0)
    }

    fn pair_count(&self, key: &str) -> usize {
        self.pairs.get(key).map_or(0, Vec::len)
    }

    fn pair_line(&self, key: &str) -> usize {
        self.pairs
            .get(key)
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

    fn record(&mut self, segments: &[String], line: usize) {
        if let Some(last) = segments.last() {
            if let Some(name) = static_of(RAW_OPS, last) {
                self.ops.entry(name).or_default().push(line);
            }
            if let Some(name) = static_of(LINK_NAMES, last) {
                self.names.entry(name).or_default().push(line);
            }
        }
        for &key in BOUNDARY_PAIRS {
            let parts: Vec<&str> = key.split("::").collect();
            if parts.len() == 2
                && segments.len() >= 2
                && segments[segments.len() - 2] == parts[0]
                && segments[segments.len() - 1] == parts[1]
            {
                self.pairs.entry(key).or_default().push(line);
            }
        }
    }
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
}

impl<'a, 'ast> Visit<'ast> for Counter<'a> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
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

/// Aliases a file's `use` declarations introduce: local name to imported path segments.
fn aliases(file: &syn::File) -> BTreeMap<String, Vec<String>> {
    let mut walker = AliasWalker::default();
    walker.visit_file(file);
    walker.aliases
}

#[derive(Default)]
struct AliasWalker {
    aliases: BTreeMap<String, Vec<String>>,
}

impl<'ast> Visit<'ast> for AliasWalker {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        collect_use(&node.tree, &mut Vec::new(), &mut self.aliases);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }
}

fn collect_use(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    aliases: &mut BTreeMap<String, Vec<String>>,
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use(&path.tree, prefix, aliases);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let mut full = prefix.clone();
            full.push(name.ident.to_string());
            aliases.insert(name.ident.to_string(), full);
        }
        UseTree::Rename(rename) => {
            let mut full = prefix.clone();
            full.push(rename.ident.to_string());
            aliases.insert(rename.rename.to_string(), full);
        }
        UseTree::Glob(_) => {}
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use(item, prefix, aliases);
            }
        }
    }
}

/// Whether the attributes gate an item on `#[cfg(test)]`.
///
/// `cfg(test)` is test-only, `cfg(all(test, ...))` is test-only when one conjunct is,
/// `cfg(any(...))` only when every alternative is, and `cfg(not(...))` never is: the item then
/// compiles outside tests too. Anything unreadable is treated as production, so it is scanned.
fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        Meta::List(list) if list.path.is_ident("cfg") => list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated)
            .map(|args| args.iter().any(cfg_is_test))
            .unwrap_or(false),
        _ => false,
    })
}

fn cfg_is_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            let Ok(args) = list
                .parse_args_with(syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("all") {
                args.iter().any(cfg_is_test)
            } else if list.path.is_ident("any") {
                !args.is_empty() && args.iter().all(cfg_is_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

/// The `#[path = "..."]` value of a module, if it has one.
fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(pair) if pair.path.is_ident("path") => match &pair.value {
            Expr::Lit(lit) => match &lit.lit {
                syn::Lit::Str(value) => Some(value.value()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    })
}

/// The directory plain `mod name;` declarations resolve in for this file.
fn module_dir(rel: &str) -> String {
    let parent = parent_dir(rel);
    let name = rel.rsplit('/').next().unwrap_or_default();
    match name {
        "lib.rs" | "main.rs" | "mod.rs" => parent,
        _ => {
            let stem = name.strip_suffix(".rs").unwrap_or(name);
            format!("{parent}{stem}/")
        }
    }
}

/// The directory containing this file, with a trailing slash.
fn parent_dir(rel: &str) -> String {
    match rel.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/"),
        None => String::new(),
    }
}

/// Follow external modules, tracking `#[cfg(test)]` and switching directory for inline modules.
struct ModuleWalker {
    plain: String,
    attr_base: String,
    tainted: bool,
    refs: Vec<ModRef>,
}

impl<'ast> Visit<'ast> for ModuleWalker {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let outer_taint = self.tainted;
        self.tainted = outer_taint || is_cfg_test(&node.attrs);
        if node.content.is_none() {
            self.refs.push(external_ref(
                node,
                &self.plain,
                &self.attr_base,
                self.tainted,
            ));
            self.tainted = outer_taint;
            return;
        }
        let inner_plain = format!("{}{}/", self.plain, node.ident);
        let outer_plain = std::mem::replace(&mut self.plain, inner_plain);
        let inner_attr = self.plain.clone();
        let outer_attr = std::mem::replace(&mut self.attr_base, inner_attr);
        syn::visit::visit_item_mod(self, node);
        self.plain = outer_plain;
        self.attr_base = outer_attr;
        self.tainted = outer_taint;
    }
}

fn external_ref(node: &ItemMod, plain: &str, attr_base: &str, cfg_test: bool) -> ModRef {
    let candidates = match path_attr(&node.attrs) {
        Some(path) => vec![normalize(&format!("{attr_base}{path}"))],
        None => {
            let name = node.ident.to_string();
            vec![
                normalize(&format!("{plain}{name}.rs")),
                normalize(&format!("{plain}{name}/mod.rs")),
            ]
        }
    };
    ModRef {
        candidates,
        cfg_test,
    }
}

fn resolve(module: &ModRef, existing: &BTreeSet<&str>) -> Vec<String> {
    module
        .candidates
        .iter()
        .filter(|candidate| existing.contains(candidate.as_str()))
        .cloned()
        .collect()
}

/// Resolve `a/b/../c` and `./` in a workspace-relative path.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
#[path = "ast_tests.rs"]
mod tests;
