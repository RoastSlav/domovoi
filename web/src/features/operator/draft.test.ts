import { clearDraft, readDraft, writeDraft } from "./draft";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

test("a_draft_survives_being_read_again", () => {
  writeDraft("c1", "Why is the NAS slow?");
  expect(readDraft("c1")).toBe("Why is the NAS slow?");
  expect(localStorage.getItem("operator:draft:c1")).toBe("Why is the NAS slow?");
  expect(readDraft("c2")).toBe("");
});

test("clearing_or_writing_empty_text_removes_the_draft", () => {
  writeDraft("c1", "x");
  writeDraft("c1", "");
  expect(readDraft("c1")).toBe("");
  writeDraft("c1", "y");
  clearDraft("c1");
  expect(readDraft("c1")).toBe("");
});

test("blocked_storage_falls_back_to_memory", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  writeDraft("c9", "kept");
  expect(readDraft("c9")).toBe("kept");
});
