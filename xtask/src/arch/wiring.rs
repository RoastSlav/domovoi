//! Module wiring: the declarations that load more Rust source (SRC-1, ADR 0023).
//!
//! Which files are compiled, and in which context, is decided by following `mod name;`,
//! `#[path = "..."]` and `include!("...")` from the Cargo target roots, never by file names;
//! [`super::reach`] does the following. This module finds the declarations in one file and
//! resolves them from the file's own location, tracking inherited `cfg(test)` gating.
//!
//! A declaration the checker cannot resolve is reported as such, never skipped: a `#[path]`
//! that is not a literal, a `cfg_attr` that sets one, an `include!` whose argument is not one
//! string literal, a path that climbs out of the workspace. `include_str!` and
//! `include_bytes!` load data, not Rust, and are not declarations.

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, Expr, ItemMod, Macro, Meta, Token};

/// What a declaration loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `mod name;`: the first candidate that exists is the module's file.
    Module(Vec<String>),
    /// `include!("path")` or a `#[path]` module: one literal path, already joined to the
    /// declaring file's directory. `None` when the path climbs out of the workspace.
    Include(Option<String>),
    /// A form the checker cannot resolve; the text says which.
    Unsupported(String),
}

/// One declaration that loads more Rust source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Line of the declaration in the declaring file.
    pub line: usize,
    /// What it loads.
    pub target: Target,
    /// True when the declaration or an enclosing module is gated by `#[cfg(test)]`.
    pub cfg_test: bool,
}

/// The loading declarations in one file, resolved from the file's own location, or `None` when
/// the file does not parse. `root` says the file is a Cargo target root: its plain modules
/// then live beside it rather than in a directory named after it.
pub fn edges_of(rel: &str, text: &str, root: bool) -> Option<Vec<Edge>> {
    let file = syn::parse_file(text).ok()?;
    let mut walker = Walker {
        dir: parent_dir(rel),
        plain: module_dir(rel, root),
        attr_base: parent_dir(rel),
        tainted: false,
        edges: Vec::new(),
    };
    walker.visit_file(&file);
    Some(walker.edges)
}

/// The directory plain `mod name;` declarations resolve in for this file.
fn module_dir(rel: &str, root: bool) -> String {
    let parent = parent_dir(rel);
    let name = rel.rsplit('/').next().unwrap_or_default();
    if root || matches!(name, "lib.rs" | "main.rs" | "mod.rs") {
        return parent;
    }
    let stem = name.strip_suffix(".rs").unwrap_or(name);
    format!("{parent}{stem}/")
}

/// The directory containing this file, with a trailing slash.
fn parent_dir(rel: &str) -> String {
    match rel.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/"),
        None => String::new(),
    }
}

/// Join `base` (a directory with a trailing slash) and `path`, resolving `.` and `..`.
/// `None` when the result would leave the workspace root.
pub fn join_checked(base: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// Follows loading declarations, tracking `#[cfg(test)]` and switching directory for inline
/// modules.
struct Walker {
    dir: String,
    plain: String,
    attr_base: String,
    tainted: bool,
    edges: Vec<Edge>,
}

impl<'ast> Visit<'ast> for Walker {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let outer_taint = self.tainted;
        self.tainted = outer_taint || is_cfg_test(&node.attrs);
        if node.content.is_none() {
            let line = node.ident.span().start().line;
            let target = module_target(node, &self.plain, &self.attr_base);
            self.edges.push(Edge {
                line,
                target,
                cfg_test: self.tainted,
            });
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

    fn visit_macro(&mut self, node: &'ast Macro) {
        if is_include(&node.path) {
            let line = node.path.span().start().line;
            let target = include_target(&node.tokens, &self.dir);
            self.edges.push(Edge {
                line,
                target,
                cfg_test: self.tainted,
            });
        } else {
            self.nested_includes(&node.tokens);
        }
        syn::visit::visit_macro(self, node);
    }
}

impl Walker {
    /// An `include!` written inside another macro's body is still an `include!`.
    fn nested_includes(&mut self, tokens: &TokenStream) {
        let flat: Vec<TokenTree> = tokens.clone().into_iter().collect();
        for (index, token) in flat.iter().enumerate() {
            if let TokenTree::Group(group) = token {
                self.nested_includes(&group.stream());
            }
            let TokenTree::Ident(ident) = token else {
                continue;
            };
            if ident != "include" {
                continue;
            }
            if let (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(body))) =
                (flat.get(index + 1), flat.get(index + 2))
                && bang.as_char() == '!'
            {
                self.edges.push(Edge {
                    line: ident.span().start().line,
                    target: include_target(&body.stream(), &self.dir),
                    cfg_test: self.tainted,
                });
            }
        }
    }
}

/// Whether a macro path is `include`, `std::include` or `core::include`.
fn is_include(path: &syn::Path) -> bool {
    let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    match names.as_slice() {
        [only] => only == "include",
        [krate, name] => (krate == "std" || krate == "core") && name == "include",
        _ => false,
    }
}

/// Resolve an `include!` body: one string literal, taken relative to the including file.
fn include_target(tokens: &TokenStream, dir: &str) -> Target {
    let literal = syn::parse2::<syn::LitStr>(tokens.clone()).or_else(|_| {
        syn::parse::Parser::parse2(
            |input: syn::parse::ParseStream<'_>| {
                let lit: syn::LitStr = input.parse()?;
                let _: Option<Token![,]> = input.parse()?;
                Ok(lit)
            },
            tokens.clone(),
        )
    });
    match literal {
        Ok(lit) => Target::Include(join_checked(dir, &lit.value())),
        Err(_) => Target::Unsupported(
            "this `include!` does not take one string literal, so the file it loads cannot be found"
                .to_owned(),
        ),
    }
}

/// Resolve an external module's candidate files, or say why they cannot be resolved.
fn module_target(node: &ItemMod, plain: &str, attr_base: &str) -> Target {
    if node.attrs.iter().any(is_cfg_attr_path) {
        return Target::Unsupported(
            "`cfg_attr` sets this module's `path`, so the file it loads cannot be found".to_owned(),
        );
    }
    let has_path = node.attrs.iter().any(|attr| attr.path().is_ident("path"));
    match (path_attr(&node.attrs), has_path) {
        (Some(path), _) => Target::Include(join_checked(attr_base, &path)),
        (None, true) => Target::Unsupported(
            "`#[path]` is not a string literal, so the file it loads cannot be found".to_owned(),
        ),
        (None, false) => {
            let name = node.ident.to_string();
            let candidates = [format!("{plain}{name}.rs"), format!("{plain}{name}/mod.rs")]
                .iter()
                .filter_map(|candidate| join_checked("", candidate))
                .collect();
            Target::Module(candidates)
        }
    }
}

/// A `cfg_attr(..., path = ...)`: the path depends on a configuration, which is not followed.
fn is_cfg_attr_path(attr: &Attribute) -> bool {
    let Meta::List(list) = &attr.meta else {
        return false;
    };
    list.path.is_ident("cfg_attr")
        && list
            .tokens
            .clone()
            .into_iter()
            .any(|token| matches!(token, TokenTree::Ident(ident) if ident == "path"))
}

/// The `#[path = "..."]` value of a module, if it has a literal one.
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

/// Whether the attributes gate an item on `#[cfg(test)]`.
///
/// `cfg(test)` is test-only, `cfg(all(test, ...))` is test-only when one conjunct is,
/// `cfg(any(...))` only when every alternative is, and `cfg(not(...))` never is: the item then
/// compiles outside tests too. Anything unreadable is treated as production, so it is scanned.
pub fn is_cfg_test(attrs: &[Attribute]) -> bool {
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
