import { useSelectedHost } from "../../app/HostContext";
import { InfoTip, PageHeader } from "../../components";
import { ChooseHost } from "./ChooseHost";
import { SkillList } from "./SkillList";
import { RISK_EXPLANATION } from "./skillText";

/**
 * The skill catalogue of the selected host, grouped by risk. A catalogue belongs to one
 * host, so under "All hosts" the page asks for one instead of guessing.
 */
export function SkillsPage() {
  const [selected] = useSelectedHost();
  return (
    <>
      <PageHeader
        title="Skills"
        docs="guide/skills.html"
        intro={
          <>
            Scripts an agent can run; write and destructive ones need your approval.{" "}
            <InfoTip title="risk levels">
              {RISK_EXPLANATION} A disabled skill stays on the host but Domovoi never runs or
              suggests it.
            </InfoTip>
          </>
        }
      />
      {selected === undefined ? <ChooseHost /> : <SkillList host={selected} />}
    </>
  );
}
