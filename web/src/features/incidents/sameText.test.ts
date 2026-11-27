import { expect, test } from "vitest";
import { sameText } from "./WhatsGoingOn";

test("repeated_fields_are_recognised_ignoring_case_and_punctuation", () => {
  expect(sameText("Disk is full.", "disk is  full")).toBe(true);
  expect(sameText("Disk is full.", "Roll back the image")).toBe(false);
  expect(sameText("", "")).toBe(false);
});
