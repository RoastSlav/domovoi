import { describeTargetFailure } from "./messages";
import { TARGET_FAILURE_CODES, UNKNOWN_FAILURE } from "./packageFailures";

describe("describeTargetFailure", () => {
  it.each(TARGET_FAILURE_CODES)("every_known_failure_code_has_words: %s", (code) => {
    const described = describeTargetFailure({ code, message: "" });
    expect(described.title.length).toBeGreaterThan(10);
    expect(described.title).not.toContain(code);
    expect(described.title).not.toBe(UNKNOWN_FAILURE);
  });

  it("uses the generic title for a code it does not know", () => {
    const described = describeTargetFailure({ code: "from_the_future", message: "odd thing" });
    expect(described).toEqual({ title: UNKNOWN_FAILURE, detail: "odd thing" });
  });

  it("strips the leading code from the message and drops a repeat of the title", () => {
    expect(
      describeTargetFailure({ code: "bad_signature", message: "bad_signature: key abc failed" }),
    ).toMatchObject({ detail: "key abc failed" });
    expect(describeTargetFailure({ code: "not_run", message: "" }).detail).toBeUndefined();
  });
});
