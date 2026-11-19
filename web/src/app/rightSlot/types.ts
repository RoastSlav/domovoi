import type { IconName } from "../../design/icons";

/** A surface's stable id. Known today: "inspector". The Operator panel adds "operator". */
export type SurfaceId = string;

/** What a surface says about itself; given to RightSlotSurface while it is mounted. */
export interface SurfaceSpec {
  id: SurfaceId;
  /** The surface's accessible name, its heading and its tab: "Step inspector", "Operator". */
  label: string;
  /** A name from design/icons.tsx, used on the tab. */
  icon: IconName;
}

/** Options for opening a surface. */
export interface OpenOptions {
  /** Where focus returns when the surface closes. Default: `document.activeElement` at the call. */
  opener?: HTMLElement | null | undefined;
  /** Move focus into the surface on opening. Default true; false for a surface opened by an address. */
  focus?: boolean | undefined;
}

/** What a page or a toolbar button may do with the right-hand slot. */
export interface RightSlotApi {
  /** Surfaces currently registered (mounted), by id. */
  specs: Readonly<Record<SurfaceId, SurfaceSpec>>;
  /** Open surfaces, oldest first: the last is the most recently opened or chosen. */
  openIds: readonly SurfaceId[];
  isOpen(id: SurfaceId): boolean;
  /** Opens `id`, or brings it to the front when it is already open. A no-op for an unregistered id. */
  open(id: SurfaceId, options?: OpenOptions): void;
  close(id: SurfaceId): void;
  toggle(id: SurfaceId, options?: OpenOptions): void;
}
