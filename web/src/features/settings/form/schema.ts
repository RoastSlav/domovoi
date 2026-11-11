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

/** The schema's description of the setting at `path`, for help text. */
export function describe(root: Schema | undefined, path: string): string | undefined {
  if (!root) {
    return undefined;
  }
  let node: Schema | undefined = root;
  let description: string | undefined;
  for (const key of path.split(".")) {
    const properties = node?.properties as Record<string, unknown> | undefined;
    const raw = properties?.[key] as Schema | undefined;
    const rawDescription = typeof raw?.description === "string" ? raw.description : undefined;
    node = resolve(root, raw);
    description =
      rawDescription ?? (typeof node?.description === "string" ? node.description : undefined);
  }
  return description;
}
