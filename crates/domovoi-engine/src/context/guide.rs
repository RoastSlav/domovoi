//! The short guide that tells the model how to read the tags on context items.

/// How to read the status, origin and cut tags on context items. It is part of every system
/// prompt whose first message carries context.
pub const CONTEXT_GUIDE: &str = include_str!("../../prompts/context_guide.md");
