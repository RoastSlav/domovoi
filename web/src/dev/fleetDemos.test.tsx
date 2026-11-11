import { screen } from "@testing-library/react";
import { renderAt } from "../test/render";
import { FleetDemos } from "./FleetDemos";

describe("the Fleet catalogue demos", () => {
  it("draws the host header, facts, checks and log lines from fixtures", () => {
    renderAt("/", "/", <FleetDemos />);
    expect(screen.getByRole("heading", { name: "nas-01" })).toBeInTheDocument();
    expect(screen.getByText("Free on /")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit disk_free" })).toBeInTheDocument();
    expect(screen.getAllByRole("log")[0]).toHaveTextContent("Cannot reach the Docker socket");
  });
});
