import { screen } from "@testing-library/react";
import { calls, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { ENTITIES } from "../../test/knowledgeFixtures";
import { renderAt } from "../../test/render";
import { subjectLabel } from "./subjects";
import { useSubjectLabels } from "./useSubjectLabels";

vi.mock("../../api/client", () => import("../../test/apiMock"));

function Labels({ refs }: { refs: string[] }) {
  const names = useSubjectLabels();
  return (
    <ul>
      {refs.map((ref, index) => (
        <li key={index}>{subjectLabel(ref, names).text}</li>
      ))}
    </ul>
  );
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/knowledge/entities", () => ENTITIES);
});

describe("useSubjectLabels", () => {
  it("subject_labels_load_host_and_entity_names_once", async () => {
    const refs = [
      "host:host-nas",
      "host:host-nas",
      "container:host-nas/plex",
      "unit:host-nas/nginx.service",
      "entity:ent-vpn",
      "entity:ent-pool",
      "entity:ent-vpn",
    ];
    renderAt("/", "/", <Labels refs={refs} />);
    expect(
      await screen.findByText("Home VPN", { selector: "li:nth-child(5)" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("nas")).toHaveLength(2);
    expect(screen.getByText("plex on nas")).toBeInTheDocument();
    expect(screen.getByText("Storage pool")).toBeInTheDocument();
    expect(calls.filter((c) => c.route === "GET /api/v1/hosts")).toHaveLength(1);
    expect(calls.filter((c) => c.route === "GET /api/v1/knowledge/entities")).toHaveLength(1);
  });
});
