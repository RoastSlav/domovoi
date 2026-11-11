import { useSkills } from "../../api/hooks/hosts";
import { Button, Dialog, EmptyState, QueryBoundary, Skeleton } from "../../components";
import { SkillBody } from "./SkillBody";

/**
 * One skill of `host` in a dialog. It reads the host's skill list (shared with the table, so
 * it is usually already loaded) and shows its own loading, error and "no longer reported"
 * states, so the dialog is never blank.
 */
export function SkillDetail({
  host,
  name,
  onClose,
}: {
  host: string;
  name: string;
  onClose: () => void;
}) {
  const skills = useSkills(host);
  return (
    <Dialog title={name} onClose={onClose} wide actions={<Button onClick={onClose}>Close</Button>}>
      <QueryBoundary
        query={skills}
        context="load the skill"
        loading={<Skeleton shape="rows" />}
        isEmpty={(list) => !list.some((skill) => skill.name === name)}
        empty={
          <EmptyState icon="tool" title="Skill not found">
            <p>{`${host} no longer reports ${name}. Close this and choose another skill.`}</p>
          </EmptyState>
        }
      >
        {(list) => {
          const skill = list.find((s) => s.name === name);
          return skill ? <SkillBody host={host} skill={skill} /> : null;
        }}
      </QueryBoundary>
    </Dialog>
  );
}
