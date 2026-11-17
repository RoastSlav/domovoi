//! The names a file's `use` declarations and type aliases introduce, so a renamed import or
//! `type Server = AgentServer;` resolves to the path it stands for (ADR 0023).

use std::collections::BTreeMap;

use syn::visit::Visit;
use syn::{ItemMod, ItemType, ItemUse, Type, UseTree};

use super::wiring::is_cfg_test;

/// Aliases a file's `use` declarations introduce: local name to imported path segments.
pub(super) fn aliases(file: &syn::File) -> BTreeMap<String, Vec<String>> {
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

    /// `type Server = AgentServer;` makes `Server` another spelling of the type.
    fn visit_item_type(&mut self, node: &'ast ItemType) {
        if let Type::Path(ty) = &*node.ty
            && ty.qself.is_none()
        {
            let mut segments: Vec<String> = ty
                .path
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
            self.aliases.insert(node.ident.to_string(), segments);
        }
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
