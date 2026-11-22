import { memo } from "react";
import { Icon } from "../../design/icons";
import { TONES, type Tone } from "../../design/tone";
import { edgeStyle } from "./edgeStyle";
import { NODE_H, NODE_W, type Point } from "./layout";
import { CHAR_W, clip, wrapLines } from "./nodeText";
import styles from "./graph.module.css";
import type { GraphEdgeModel, GraphNodeModel } from "./types";

const KNOWN_TONES = new Set<string>(TONES);

/** The tone a node is drawn in; a tone this version does not know is neutral. */
export function toneOf(tone: string): Tone {
  return KNOWN_TONES.has(tone) ? (tone as Tone) : "neutral";
}

/** The `transform` attribute that puts a node's corner at a point. */
export function translate(x: number, y: number): string {
  return `translate(${x} ${y})`;
}

/** The `d` attribute of a polyline. */
export function pathOf(points: readonly Point[]): string {
  return points.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x} ${y}`).join(" ");
}

/** One node: a card, its kind, its outcome and a two-line summary, all drawn as SVG text. */
export const NodeShape = memo(function NodeShape(props: {
  node: GraphNodeModel;
  corner: Point;
  selected: boolean;
  focused: boolean;
  settle: boolean;
  onSelect(id: string): void;
  onOpen(id: string): void;
}) {
  const { node } = props;
  const group = node.kind === "group";
  const tone = group ? "neutral" : toneOf(node.status.tone);
  const retry = node.retry ? 7 : 0;
  const statusChars = Math.ceil(node.status.label.length * 1.05);
  const titleRoom = Math.floor(
    (NODE_W - 12 - 34 - 12 - 14 - 6 - statusChars * CHAR_W) / CHAR_W - retry,
  );
  const lines = node.summary ? wrapLines(node.summary, 31, 2) : [];
  const classes = [styles.node, styles[`tone-${tone}`], props.settle ? styles.settle : ""];
  return (
    <g
      className={classes.filter(Boolean).join(" ")}
      data-node-id={node.id}
      data-kind={node.kind}
      transform={translate(props.corner[0], props.corner[1])}
      onClick={() => props.onSelect(node.id)}
      onDoubleClick={() => props.onOpen(node.id)}
    >
      <rect className={styles.card} width={NODE_W} height={NODE_H} rx={8} />
      {group && (
        <rect
          className={styles.groupBorder}
          width={NODE_W}
          height={NODE_H}
          rx={8}
          strokeDasharray="5 4"
        />
      )}
      <g transform="translate(12 8)">
        <Icon name={node.icon} size={16} />
      </g>
      <text className={styles.title} x={34} y={21}>
        {clip(node.title, titleRoom)}
        {node.retry && (
          <tspan className={styles.retry} dx={6}>
            Retry
          </tspan>
        )}
      </text>
      <g transform={`translate(${NODE_W - 26} 9)`}>
        <Icon name={node.status.icon} size={14} />
      </g>
      <text className={styles.status} x={NODE_W - 30} y={21} textAnchor="end">
        {node.status.label}
      </text>
      {lines.map((line, index) => (
        <text key={index} className={styles.summary} x={12} y={43 + index * 15}>
          {line}
        </text>
      ))}
      {props.selected && (
        <rect
          className={styles.selected}
          x={1.5}
          y={1.5}
          width={NODE_W - 3}
          height={NODE_H - 3}
          rx={7}
        />
      )}
      {props.focused && (
        <rect
          className={styles.focus}
          x={-3}
          y={-3}
          width={NODE_W + 6}
          height={NODE_H + 6}
          rx={11}
        />
      )}
    </g>
  );
});

/** One edge: a line with an arrowhead at the dependent; the dash pattern says the kind. */
export const EdgeShape = memo(function EdgeShape(props: {
  edge: GraphEdgeModel;
  route: readonly Point[];
  marker: string;
  labelled: boolean;
}) {
  const { edge, route } = props;
  if (route.length < 2) {
    return null;
  }
  const style = edgeStyle(edge.kind);
  const mid = route[Math.floor(route.length / 2)] ?? route[0];
  return (
    <g data-edge-group={edge.id}>
      <path
        className={styles.edge}
        data-edge-id={edge.id}
        d={pathOf(route)}
        strokeWidth={style.weight}
        strokeDasharray={style.dash}
        markerEnd={`url(#${props.marker})`}
      />
      {props.labelled && mid && (
        <text className={styles.edgeLabel} x={mid[0]} y={mid[1] - 5} textAnchor="middle">
          {edge.label}
        </text>
      )}
    </g>
  );
});
