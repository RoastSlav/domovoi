import { experienceRequest, readExperienceView, writeExperienceView } from "./experienceParams";

describe("experience params", () => {
  it("experience_params_default_to_host_scope_and_never_send_a_host_for_global", () => {
    expect(readExperienceView(new URLSearchParams())).toEqual({ scope: "host", q: "" });
    expect(readExperienceView(new URLSearchParams("scope=bogus"))).toEqual({
      scope: "host",
      q: "",
    });
    expect(readExperienceView(new URLSearchParams("scope=global&q=plex"))).toEqual({
      scope: "global",
      q: "plex",
    });

    expect(experienceRequest({ scope: "host", q: "plex" }, "nas")).toEqual({
      scope: "host",
      q: "plex",
      host: "nas",
    });
    // No selected host: no request at all.
    expect(experienceRequest({ scope: "host", q: "" }, undefined)).toBeUndefined();
    // A global search drops the host.
    const global = experienceRequest({ scope: "global", q: "plex" }, "nas");
    expect(global).toEqual({ scope: "global", q: "plex" });
    expect("host" in (global ?? {})).toBe(false);
    // A blank search is valid.
    expect(experienceRequest({ scope: "global", q: "" }, undefined)).toEqual({
      scope: "global",
      q: "",
    });
  });

  it("writing the address keeps other parameters and drops defaults", () => {
    const base = new URLSearchParams("host=nas");
    const global = writeExperienceView(base, { scope: "global", q: "plex" });
    expect(global.get("scope")).toBe("global");
    expect(global.get("q")).toBe("plex");
    expect(global.get("host")).toBe("nas");
    const back = writeExperienceView(global, { scope: "host", q: "" });
    expect(back.toString()).toBe("host=nas");
  });
});
