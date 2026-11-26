import { Stack, Text } from "../../components";

/** The management paths the hub could not verify the block avoids: "Not protected". */
export function UnprotectedList({ paths }: { paths: readonly string[] }) {
  if (paths.length === 0) {
    return (
      <Text muted block size="sm">
        The hub found no management path to list, and it could not verify any.
      </Text>
    );
  }
  return (
    <Stack gap={1}>
      <Text strong>Not protected</Text>
      <ul aria-label="Paths not protected">
        {paths.map((path) => (
          <li key={path}>
            <code>{path}</code>
          </li>
        ))}
      </ul>
    </Stack>
  );
}
