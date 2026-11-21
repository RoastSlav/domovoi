import { formatSubject, parseSubject, subjectLabel, type SubjectNames } from "./subjects";

const names: SubjectNames = {
  hosts: new Map([["host-nas", "nas"]]),
  entities: new Map([["abc", "Home VPN"]]),
};

describe("subjects", () => {
  it("subjects_round_trip_and_never_assume_a_uuid", () => {
    for (const ref of [
      "host:host-nas",
      "container:host-nas/plex",
      "unit:host-nas/nginx.service",
      "entity:abc",
    ]) {
      const parsed = parseSubject(ref);
      expect(parsed).toBeDefined();
      expect(parsed && formatSubject(parsed)).toBe(ref);
    }
    expect(parseSubject("container:host-nas/a/b")).toEqual({
      kind: "container",
      host: "host-nas",
      name: "a/b",
    });
    expect(parseSubject("x:y")).toBeUndefined();
    expect(parseSubject("host")).toBeUndefined();
    expect(parseSubject("")).toBeUndefined();
    expect(parseSubject("host:")).toBeUndefined();
    expect(parseSubject("container:no-slash")).toBeUndefined();
  });

  it("subject_labels_use_names_and_fall_back_without_throwing", () => {
    expect(subjectLabel("host:host-nas", names)).toEqual({ text: "nas", to: "/hosts/nas" });
    expect(subjectLabel("container:host-nas/plex", names)).toEqual({
      text: "plex on nas",
      to: "/hosts/nas",
    });
    expect(subjectLabel("entity:abc", names)).toEqual({
      text: "Home VPN",
      to: "/knowledge/entities/abc",
    });
    expect(subjectLabel("host:gone", names).text).toBe("an unknown host");
    expect(subjectLabel("unit:gone/x.service", names).text).toBe("x.service on an unknown host");
    expect(subjectLabel("entity:gone", names).text).toBe("a removed entity");
    expect(subjectLabel("<b>nope</b>", names)).toEqual({ text: "<b>nope</b>" });
  });
});
