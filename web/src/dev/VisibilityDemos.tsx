import { DetailsScope, Disclosure, Visible } from "../components";
import { PROFILE_IDS, PROFILE_META } from "../design/visibility";
import { ProfileProvider } from "../test/visibility";
import { Demo, Group } from "./Demo";

/** One small screen: a P0 line, a P1 line, a P2 block and a P3 fact, drawn at a given level. */
function Sample() {
  return (
    <>
      <Visible category="P0" id="demo.state">
        <p>Waiting for your approval.</p>
      </Visible>
      <Visible category="P1" id="demo.main">
        <p>Restart plex on nas-01.</p>
      </Visible>
      <Visible category="P2" id="demo.technical" label="Step output" count={1}>
        <p>exit 0, 3 lines</p>
      </Visible>
      <DetailsScope>
        <Visible category="P3" id="demo.metadata" label="Kind and times" count={2}>
          <p>Kind: investigation. Started 2 minutes ago.</p>
        </Visible>
      </DetailsScope>
    </>
  );
}

/** The visibility kit at each detail level, plus a bare `Disclosure`. */
export function VisibilityDemos() {
  return (
    <Group title="Detail levels">
      {PROFILE_IDS.map((profile) => (
        <Demo key={profile} name={`Visible at ${PROFILE_META[profile].label}`}>
          <ProfileProvider profile={profile}>
            <Sample />
          </ProfileProvider>
        </Demo>
      ))}
      <Demo name="Disclosure">
        <Disclosure label="Connections" count={3} summary="Evidence, plan">
          <p>Three linked steps.</p>
        </Disclosure>
      </Demo>
    </Group>
  );
}
