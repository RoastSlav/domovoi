type Schema = Record<string, unknown>;

function resolve(root: Schema, node: unknown): Schema | undefined {
  if (typeof node !== "object" || node === null) {
    return undefined;
  }
  const schema = node as Schema;
  const ref = schema.$ref;
  if (typeof ref === "string") {
    const name = ref.split("/").pop() ?? "";
    const defs = (root.$defs ?? root.definitions) as Record<string, Schema> | undefined;
    return resolve(root, defs?.[name]);
  }
  for (const key of ["anyOf", "oneOf", "allOf"]) {
    const options = schema[key];
    if (Array.isArray(options) && !schema.properties) {
      for (const option of options) {
        const found = resolve(root, option);
        if (found && (found.properties || found.description)) {
          return { description: schema.description, ...found };
        }
      }
    }
  }
  return schema;
}

/** The schema node at `path`: as written in its parent, and with `$ref` and `anyOf` resolved. */
function walk(root: Schema, path: string): { raw?: Schema; node?: Schema } {
  let node: Schema | undefined = root;
  let raw: Schema | undefined;
  for (const key of path.split(".")) {
    const properties = node?.properties as Record<string, unknown> | undefined;
    raw = properties?.[key] as Schema | undefined;
    node = resolve(root, raw);
  }
  return { ...(raw ? { raw } : {}), ...(node ? { node } : {}) };
}

/**
 * Rustdoc written for developers, as a sentence for the person reading the page: inline code
 * loses its backticks, links keep their text, and line breaks become spaces.
 */
export function tidyDescription(text: string): string {
  return text
    .replace(/\[`?([^\]`]+)`?\]\([^)]*\)/g, "$1")
    .replace(/\[`([^\]`]+)`\]/g, "$1")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/** The schema's description of the setting at `path`, for help text. */
export function describe(root: Schema | undefined, path: string): string | undefined {
  if (!root) {
    return undefined;
  }
  const { raw, node } = walk(root, path);
  const text =
    typeof raw?.description === "string"
      ? raw.description
      : typeof node?.description === "string"
        ? node.description
        : undefined;
  const tidy = text === undefined ? undefined : tidyDescription(text);
  return tidy || undefined;
}

/**
 * The schema's default for the setting at `path` as text for an empty input's placeholder.
 * Only strings and numbers have one: an object, a list or a switch shows no placeholder.
 */
export function defaultOf(root: Schema | undefined, path: string): string | undefined {
  if (!root) {
    return undefined;
  }
  const { raw, node } = walk(root, path);
  const value = raw && "default" in raw ? raw.default : node?.default;
  if (typeof value === "number" || (typeof value === "string" && value !== "")) {
    return String(value);
  }
  return undefined;
}

/** The schema's default for a switch at `path`, or `undefined` when it has none. */
export function booleanDefaultOf(root: Schema | undefined, path: string): boolean | undefined {
  if (!root) {
    return undefined;
  }
  const { raw, node } = walk(root, path);
  const value = raw && "default" in raw ? raw.default : node?.default;
  return typeof value === "boolean" ? value : undefined;
}
