import { useSearchParams } from "react-router";
import { useFacts } from "../../api/hooks/knowledge";
import { useSelectedHost } from "../../app/HostContext";
import { PageHeader, Tabs, VisibilityPage, type Tab } from "../../components";
import { EntitiesTab } from "./EntitiesTab";
import { FactsTab } from "./FactsTab";
import { ProposalsTab } from "./ProposalsTab";
import { RelationsTab } from "./RelationsTab";

const TABS = ["facts", "proposals", "relations", "entities"] as const;
type TabId = (typeof TABS)[number];

/** The tab in `?tab=`: no parameter, or one this page does not know, is the facts list. */
export function readTab(params: URLSearchParams): TabId {
  const tab = params.get("tab");
  return TABS.find((id) => id === tab) ?? "facts";
}

/** What Domovoi knows: facts, the proposals that wait for the owner, relations and entities. */
export function KnowledgePage() {
  const [params, setParams] = useSearchParams();
  const [host] = useSelectedHost();
  // The same query the Proposals tab and the note on the Facts tab use, so it is one request.
  const proposals = useFacts({ source: "inferred", host });
  const waiting = proposals.data?.pages.reduce((total, page) => total + page.items.length, 0) ?? 0;

  const tabs: Tab[] = [
    { id: "facts", label: "Facts", content: <FactsTab waiting={waiting} /> },
    {
      id: "proposals",
      label: "Proposals",
      ...(waiting > 0
        ? { count: { value: waiting, label: "waiting", tone: "pending" as const } }
        : {}),
      content: <ProposalsTab />,
    },
    { id: "relations", label: "Relations", content: <RelationsTab /> },
    { id: "entities", label: "Entities", content: <EntitiesTab /> },
  ];

  return (
    <VisibilityPage page="knowledge">
      <PageHeader
        title="Knowledge"
        intro="What Domovoi knows about your servers, where each fact came from, and what waits for your word."
        docs="concepts/knowledge.html"
      />
      <Tabs
        label="Knowledge"
        tabs={tabs}
        selected={readTab(params)}
        onSelect={(id) =>
          setParams(
            (previous) => {
              const next = new URLSearchParams(previous);
              if (id === "facts") {
                next.delete("tab");
              } else {
                next.set("tab", id);
              }
              return next;
            },
            { replace: true },
          )
        }
      />
    </VisibilityPage>
  );
}
