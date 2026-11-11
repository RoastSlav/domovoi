import { useState } from "react";
import type { AckRule } from "../../api/types";
import { Menu } from "../../components";
import { DeleteRuleDialog } from "./DeleteRuleDialog";
import { hostWords } from "./phrase";

/** The actions of one known condition behind a "More" button: delete, which asks first. */
export function RuleRowMenu({ rule }: { rule: AckRule }) {
  const [deleting, setDeleting] = useState(false);
  return (
    <>
      <Menu
        label={`Actions for ${rule.check} on ${hostWords(rule.host)}`}
        items={[
          { label: "Delete", icon: "trash", tone: "danger", onSelect: () => setDeleting(true) },
        ]}
      />
      {deleting && <DeleteRuleDialog rule={rule} onClose={() => setDeleting(false)} />}
    </>
  );
}
