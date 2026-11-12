import { useState } from "react";
import { useCheckUpdate, useUpdate } from "../../api/hooks/updates";
import {
  Button,
  Card,
  Cluster,
  KeyValueList,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Stack,
  Tag,
  Text,
  useActionFeedback,
} from "../../components";
import { HowToUpdateDialog } from "./HowToUpdateDialog";

/**
 * The hub's version, the newest release it knows of, when it last looked, and a button that
 * looks now. When a newer release exists, how to install it is one button away.
 */
export function UpdateAbout() {
  const update = useUpdate();
  const [showing, setShowing] = useState(false);
  const check = useActionFeedback(useCheckUpdate(), {
    success: (found) =>
      found.newer && found.latest
        ? `Domovoi ${found.latest} is available`
        : { title: "You are on the latest version", tone: "info" },
    error: "check for updates",
  });
  return (
    <QueryBoundary
      query={update}
      context="load the release check"
      loading={<Skeleton shape="rows" count={3} />}
    >
      {(info) => (
        <Stack gap={3}>
          <Card>
            <KeyValueList
              items={[
                {
                  label: "Version",
                  value: (
                    <Cluster gap={2}>
                      <Text strong>{info.current}</Text>
                      {info.newer && info.latest ? (
                        <Tag tone="info">{`Domovoi ${info.latest} is available`}</Tag>
                      ) : (
                        info.latest && <Tag tone="success">Up to date</Tag>
                      )}
                    </Cluster>
                  ),
                },
                {
                  label: "Last checked",
                  value: info.checked_at ? <RelativeTime at={info.checked_at} /> : "never",
                },
              ]}
            />
          </Card>
          <Cluster>
            <Button icon="refresh" onClick={() => check.mutate()} loading={check.isPending}>
              Check now
            </Button>
            {info.newer && info.latest && (
              <Button variant="primary" onClick={() => setShowing(true)}>
                How to update
              </Button>
            )}
          </Cluster>
          {showing && <HowToUpdateDialog onClose={() => setShowing(false)} />}
        </Stack>
      )}
    </QueryBoundary>
  );
}
