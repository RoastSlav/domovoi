import { useState } from "react";
import type { AckRule } from "../../api/types";
import { Menu } from "../../components";
import { DeleteRuleDialog } from "./DeleteRuleDialog";
import { EditRuleDialog } from "./EditRuleDialog";
import { hostWords } from "./phrase";

/** The actions of one known condition behind a "More" button: edit, and delete, which asks first. */
export function RuleRowMenu({ rule }: { rule: AckRule }) {
  const [deleting, setDeleting] = useState(false);
  const [editing, setEditing] = useState(false);
  return (
    <>
      <Menu
        label={`Actions for ${rule.check} on ${hostWords(rule.host)}`}
        items={[
          { label: "Edit", icon: "edit", onSelect: () => setEditing(true) },
          { label: "Delete", icon: "trash", tone: "danger", onSelect: () => setDeleting(true) },
        ]}
      />
      {editing && <EditRuleDialog rule={rule} onClose={() => setEditing(false)} />}
      {deleting && <DeleteRuleDialog rule={rule} onClose={() => setDeleting(false)} />}
    </>
  );
}
