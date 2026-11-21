import { useSearchParams } from "react-router";
import { Tabs, type Tab } from "../../components";
import { PackagesTab } from "../packages/PackagesTab";
import { HelperTab } from "./HelperTab";
import { InventoryTab } from "./InventoryTab";
import { LimitsTab } from "./LimitsTab";
import styles from "./capabilityCenter.module.css";

const TAB_IDS = ["inventory", "packages", "limits", "helper"] as const;

const TABS: Tab[] = [
  { id: "inventory", label: "Inventory", content: <InventoryTab /> },
  { id: "packages", label: "Packages", content: <PackagesTab /> },
  { id: "limits", label: "Host limits", content: <LimitsTab /> },
  { id: "helper", label: "Helper", content: <HelperTab /> },
];

/** The Capability Center's tabs, the open one kept in `?tab=` (Inventory, the first, has none). */
export function CapabilityTabs() {
  const [params, setParams] = useSearchParams();
  const asked = params.get("tab");
  const selected = TAB_IDS.find((id) => id === asked) ?? TAB_IDS[0];
  return (
    <div className={styles.tabs}>
      <Tabs
        label="Capability Center"
        tabs={TABS}
        selected={selected}
        onSelect={(id) => {
          const next = new URLSearchParams(params);
          if (id === TAB_IDS[0]) {
            next.delete("tab");
          } else {
            next.set("tab", id);
          }
          setParams(next, { replace: true });
        }}
      />
    </div>
  );
}
