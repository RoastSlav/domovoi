import { useHostNames } from "../../api/hooks/hosts";
import { usePolicyVersion } from "../../api/hooks/policy";
import { AllHostsNote } from "../../app/AllHostsNote";
import { Loading, QueryBoundary, Text, Visible } from "../../components";
import { RulesTable } from "./RulesTable";

/**
 * The rules of the active version, read-only, loaded on their own so a failure here leaves the
 * rest of the page standing.
 * @param number The active version, or undefined when none is published.
 * @param zone The hub's time-window zone.
 */
export function ActiveRules({ number, zone }: { number: number | undefined; zone: string }) {
  const hostNames = useHostNames();
  const version = usePolicyVersion(number);
  if (number === undefined) {
    return (
      <Visible category="P0" id="policy.rules">
        <Text muted>There are no rules because no policy is published yet.</Text>
      </Visible>
    );
  }
  return (
    <Visible category="P0" id="policy.rules">
      <AllHostsNote>Policy applies to every host. A rule names a host itself.</AllHostsNote>
      <QueryBoundary query={version} context="load the rules" loading={<Loading />}>
        {(detail) => <RulesTable rules={detail.rules} hostNames={hostNames} zone={zone} />}
      </QueryBoundary>
    </Visible>
  );
}
