import { render, screen } from "@testing-library/react";
import { HomePage } from "./HomePage";

describe("HomePage", () => {
  it("renders the product name", () => {
    render(<HomePage />);
    expect(screen.getByRole("heading", { name: "Domovoi" })).toBeInTheDocument();
  });
});
