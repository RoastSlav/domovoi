//! The hub's module-edge rules (MOD-1 to MOD-4, ADR 0026).
//!
//! The hub has three transports (HTTP, chat, webhooks) and a use-case layer. Transports call
//! services; they never import each other, and a service never imports a transport or the
//! runtime. Every file is parsed with `syn` and every path it mentions is resolved to a module
//! of the hub, so a `super::` chain, a renamed import, a qualified path in an expression or a
//! macro body cannot hide an edge. A file that cannot be parsed is a violation, never a pass.
//!
//! This is the one implementation: `cargo xtask arch-check` and the hub's gate test call
//! [`module_edge_violations`].

use std::collections::BTreeMap;

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    Ident, ItemConst, ItemEnum, ItemFn, ItemImpl, ItemMod, ItemStatic, ItemStruct, ItemTrait,
    ItemType, ItemUse, Macro, Path, UseTree,
};

use super::Violation;
use super::alias::aliases;
use super::rules::{
    HUB_SRC, SERVICE_FORBIDDEN_MODULES, TRANSPORT_FORBIDDEN_NAMES, UNZONED_MODULES, ZONES, Zone,
    ZoneKind,
};
use super::wiring::is_cfg_test;

/// Every module-edge violation in the hub's production files. `files` holds
/// `(workspace-relative path, contents)` pairs; files outside the hub are ignored.
pub fn module_edge_violations(files: &[(String, String)]) -> Vec<Violation> {
    let mut out = Vec::new();
    for (rel, text) in files {
        let Some(below) = rel.strip_prefix(HUB_SRC) else {
            continue;
        };
        let module = module_of(below);
        let file = match syn::parse_file(text) {
            Ok(file) => file,
            Err(error) => {
                out.push(Violation {
                    rule: "MOD-1",
                    location: rel.clone(),
                    message: format!(
                        "cannot parse this file, so its module edges cannot be checked (fail-closed): {error}"
                    ),
                });
                continue;
            }
        };
        if let Some(top) = module.first()
            && !ZONES.iter().any(|zone| first_segment(zone) == top)
            && !UNZONED_MODULES.contains(&top.as_str())
        {
            out.push(Violation {
                rule: "MOD-3",
                location: rel.clone(),
                message: "a new top-level hub module must be added to `ZONES` (a transport or the services) or `UNZONED_MODULES`, with a reason in the ADR".to_owned(),
            });
        }
        let aliases = aliases(&file);
        let mut walker = Walker {
            module: module.clone(),
            zone: zone_of(&module),
            aliases: &aliases,
            found: Vec::new(),
            names: Vec::new(),
        };
        walker.visit_file(&file);
        let here = walker.zone;
        for (target, line) in walker.found {
            if let Some(rule) = edge_rule(here, &target) {
                out.push(Violation {
                    rule: rule.0,
                    location: format!("{rel}:{line}"),
                    message: format!("{} (`crate::{}`)", rule.1, target.join("::")),
                });
            }
        }
        if matches!(here, Some(zone) if zone.kind == ZoneKind::Transport) {
            for (name, line) in walker.names {
                out.push(Violation {
                    rule: "MOD-4",
                    location: format!("{rel}:{line}"),
                    message: format!(
                        "a transport does not hold a command handle; call the service that owns the command (`{name}`)"
                    ),
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.location, a.rule).cmp(&(&b.location, b.rule)));
    out.dedup();
    out
}

/// The rule a reference from a file in zone `here` to module path `target` breaks, if any.
fn edge_rule(
    here: Option<&'static Zone>,
    target: &[String],
) -> Option<(&'static str, &'static str)> {
    let here = here?;
    let there = zone_of(target);
    match here.kind {
        ZoneKind::Transport => {
            let there = there?;
            (there.kind == ZoneKind::Transport && there.name != here.name).then_some((
                "MOD-1",
                "transports do not import each other; both call a service in `crate::services`",
            ))
        }
        ZoneKind::Service => {
            let transport = there.is_some_and(|zone| zone.kind == ZoneKind::Transport);
            let runtime = target
                .first()
                .is_some_and(|top| SERVICE_FORBIDDEN_MODULES.contains(&top.as_str()));
            (transport || runtime).then_some((
                "MOD-2",
                "a service does not import a transport or the runtime; move the shared type out of the transport",
            ))
        }
    }
}

fn first_segment(zone: &Zone) -> &str {
    zone.module.split("::").next().unwrap_or_default()
}

/// A file's module path below the crate root: `api/actions.rs` is `[api, actions]`,
/// `auth/interactions/mod.rs` is `[auth, interactions]`, `lib.rs` is `[]`.
fn module_of(below: &str) -> Vec<String> {
    let stem = below.strip_suffix(".rs").unwrap_or(below);
    let mut parts: Vec<String> = stem.split('/').map(str::to_owned).collect();
    if parts
        .last()
        .is_some_and(|last| last == "mod" || last == "lib")
    {
        parts.pop();
    }
    parts
}

/// The zone with the longest `module` that is a prefix of `module`.
fn zone_of(module: &[String]) -> Option<&'static Zone> {
    ZONES
        .iter()
        .filter(|zone| {
            let parts: Vec<&str> = zone.module.split("::").collect();
            module.len() >= parts.len() && module.iter().zip(&parts).all(|(a, b)| a == b)
        })
        .max_by_key(|zone| zone.module.split("::").count())
}

/// Collects the hub modules a file mentions, and the forbidden handle names it writes.
struct Walker<'a> {
    /// The module being visited: the file's, plus any inline modules entered so far.
    module: Vec<String>,
    zone: Option<&'static Zone>,
    aliases: &'a BTreeMap<String, Vec<String>>,
    /// Resolved module paths below `crate`, with the line each was written on.
    found: Vec<(Vec<String>, usize)>,
    names: Vec<(&'static str, usize)>,
}

impl Walker<'_> {
    /// Resolves `segments` as written in this file to a path below `crate`, or `None` for an
    /// external crate or a local item.
    fn resolve(&self, segments: &[String]) -> Option<Vec<String>> {
        let first = segments.first()?;
        let mut path: Vec<String> = match self.aliases.get(first) {
            Some(full) if !matches!(first.as_str(), "crate" | "self" | "super") => {
                let mut expanded = full.clone();
                expanded.extend(segments[1..].iter().cloned());
                expanded
            }
            _ => segments.to_vec(),
        };
        match path.first().map(String::as_str) {
            Some("crate") => {
                path.remove(0);
                Some(path)
            }
            Some("self") => {
                path.remove(0);
                let mut full = self.module.clone();
                full.extend(path);
                Some(full)
            }
            Some("super") => {
                let mut base = self.module.clone();
                while path.first().is_some_and(|s| s == "super") {
                    path.remove(0);
                    base.pop()?;
                }
                base.extend(path);
                Some(base)
            }
            _ => None,
        }
    }

    fn record(&mut self, segments: &[String], line: usize) {
        if let Some(path) = self.resolve(segments) {
            self.found.push((path, line));
        }
    }

    fn record_name(&mut self, ident: &str, line: usize) {
        if let Some(name) = TRANSPORT_FORBIDDEN_NAMES.iter().find(|n| **n == ident) {
            self.names.push((name, line));
        }
    }

    /// Every leaf of a `use` tree, with groups, renames and `self` expanded.
    fn use_leaves(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.use_leaves(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => {
                let mut full = prefix.clone();
                if name.ident != "self" {
                    full.push(name.ident.to_string());
                }
                self.record(&full, name.ident.span().start().line);
                self.record_name(&name.ident.to_string(), name.ident.span().start().line);
            }
            UseTree::Rename(rename) => {
                let mut full = prefix.clone();
                full.push(rename.ident.to_string());
                self.record(&full, rename.ident.span().start().line);
                self.record_name(&rename.ident.to_string(), rename.ident.span().start().line);
            }
            UseTree::Glob(glob) => self.record(prefix, glob.span().start().line),
            UseTree::Group(group) => {
                for item in &group.items {
                    self.use_leaves(item, prefix);
                }
            }
        }
    }

    /// Paths and names inside a macro body: `a :: b :: c` runs of identifiers, and any
    /// identifier that is a forbidden handle name.
    fn scan_tokens(&mut self, tokens: &TokenStream) {
        let flat: Vec<TokenTree> = tokens.clone().into_iter().collect();
        let mut i = 0;
        while i < flat.len() {
            match &flat[i] {
                TokenTree::Group(group) => self.scan_tokens(&group.stream()),
                TokenTree::Ident(ident) => {
                    let line = ident.span().start().line;
                    self.record_name(&ident.to_string(), line);
                    let mut run = vec![ident.to_string()];
                    let mut j = i + 1;
                    while let (
                        Some(TokenTree::Punct(a)),
                        Some(TokenTree::Punct(b)),
                        Some(TokenTree::Ident(next)),
                    ) = (flat.get(j), flat.get(j + 1), flat.get(j + 2))
                    {
                        if a.as_char() != ':' || b.as_char() != ':' {
                            break;
                        }
                        self.record_name(&next.to_string(), next.span().start().line);
                        run.push(next.to_string());
                        j += 3;
                    }
                    if run.len() > 1 || self.aliases.contains_key(&run[0]) {
                        self.record(&run, line);
                    }
                    i = j;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }
}

impl<'ast> Visit<'ast> for Walker<'_> {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        self.use_leaves(&node.tree, &mut Vec::new());
    }

    fn visit_path(&mut self, node: &'ast Path) {
        let segments: Vec<String> = node.segments.iter().map(|s| s.ident.to_string()).collect();
        self.record(&segments, node.span().start().line);
        syn::visit::visit_path(self, node);
    }

    fn visit_ident(&mut self, node: &'ast Ident) {
        self.record_name(&node.to_string(), node.span().start().line);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        self.scan_tokens(&node.tokens);
        syn::visit::visit_macro(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if !is_cfg_test(&node.attrs) {
            self.module.push(node.ident.to_string());
            syn::visit::visit_item_mod(self, node);
            self.module.pop();
        }
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_fn(self, node);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_impl(self, node);
        }
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_struct(self, node);
        }
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_enum(self, node);
        }
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_trait(self, node);
        }
    }

    fn visit_item_type(&mut self, node: &'ast ItemType) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_type(self, node);
        }
    }

    fn visit_item_const(&mut self, node: &'ast ItemConst) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_const(self, node);
        }
    }

    fn visit_item_static(&mut self, node: &'ast ItemStatic) {
        if !is_cfg_test(&node.attrs) {
            syn::visit::visit_item_static(self, node);
        }
    }
}

#[cfg(test)]
#[path = "modules_tests.rs"]
mod tests;
