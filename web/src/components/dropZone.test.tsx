import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { acceptsFile, DropZone } from "./DropZone";

const md = new File(["# a"], "a.md", { type: "text/markdown" });
const png = new File(["x"], "b.png", { type: "image/png" });

test("dropzone_hands_over_the_files_chosen_with_its_real_file_input", async () => {
  const onFiles = vi.fn();
  render(<DropZone label="Choose files or drop them here" onFiles={onFiles} />);

  await userEvent.upload(screen.getByLabelText("Choose files or drop them here"), [md, png]);

  expect(onFiles).toHaveBeenCalledWith([md, png]);
});

test("dropzone_keeps_only_dropped_files_that_match_accept", () => {
  const onFiles = vi.fn();
  render(<DropZone label="Drop here" accept=".md,text/markdown" onFiles={onFiles} />);

  fireEvent.drop(screen.getByText("Drop here"), { dataTransfer: { files: [md, png] } });

  expect(onFiles).toHaveBeenCalledWith([md]);
});

test("dropzone_ignores_drops_and_disables_its_input_while_blocked", () => {
  const onFiles = vi.fn();
  render(<DropZone label="Drop here" disabledReason="Wait for the import." onFiles={onFiles} />);

  fireEvent.drop(screen.getByText("Drop here"), { dataTransfer: { files: [md] } });

  expect(onFiles).not.toHaveBeenCalled();
  expect(screen.getByLabelText("Drop here")).toBeDisabled();
});

test("accepts_file_matches_extensions_and_media_types_ignoring_case", () => {
  expect(acceptsFile(new File([""], "NOTES.MD"), ".md")).toBe(true);
  expect(acceptsFile(png, "image/*")).toBe(true);
  expect(acceptsFile(png, ".md,text/markdown")).toBe(false);
  expect(acceptsFile(png, undefined)).toBe(true);
});
