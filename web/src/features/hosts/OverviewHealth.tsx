import { Section, StatTile, Tooltip } from "../../components";
import { Stale, Updated } from "./Freshness";
import type { HealthTile } from "./healthTiles";
import { TileGrid } from "./TileGrid";

/** What a tile says under its number: the hint, with a tooltip for the sensors, and a stale mark. */
function Hint({ tile }: { tile: HealthTile }) {
  const text = tile.tip ? (
    <Tooltip content={tile.tip}>
      <span tabIndex={0}>{tile.hint}</span>
    </Tooltip>
  ) : (
    tile.hint
  );
  return (
    <>
      {text}
      <Stale at={tile.at} />
    </>
  );
}

/** CPU, load, memory, swap, uptime and temperature as one row of tiles, toned by how bad they are. */
export function OverviewHealth({ tiles }: { tiles: HealthTile[] }) {
  if (tiles.length === 0) {
    return null;
  }
  return (
    <Section
      title="Health"
      description={<Updated facts={tiles.map((tile) => ({ observed_at: tile.at }))} />}
    >
      <TileGrid
        tiles={tiles.map((tile) => (
          <StatTile
            key={tile.label}
            label={tile.label}
            value={tile.value}
            tone={tile.tone}
            hint={<Hint tile={tile} />}
          />
        ))}
      />
    </Section>
  );
}
