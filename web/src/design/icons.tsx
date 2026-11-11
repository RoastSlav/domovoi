import {
  Ban,
  BellOff,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  CircleAlert,
  CircleCheck,
  CircleQuestionMark,
  CircleX,
  Clock,
  Copy,
  Ellipsis,
  ExternalLink,
  Eye,
  EyeOff,
  FileText,
  Funnel,
  Info,
  ListChecks,
  LoaderCircle,
  LogOut,
  Menu,
  MessageSquare,
  Monitor,
  Moon,
  Newspaper,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings,
  SquareTerminal,
  Sun,
  Trash2,
  TriangleAlert,
  User,
  WifiOff,
  Wrench,
  X,
  type LucideIcon,
} from "lucide-react";

/** Every icon the UI may draw. Add a name here before using a new icon anywhere. */
const ICONS = {
  // Navigation
  alert: TriangleAlert,
  chat: MessageSquare,
  rules: ListChecks,
  approve: CircleCheck,
  server: Server,
  tool: Wrench,
  note: FileText,
  digest: Newspaper,
  log: SquareTerminal,
  gear: Settings,
  menu: Menu,
  user: User,
  logout: LogOut,
  // Status
  check: Check,
  x: X,
  "check-circle": CircleCheck,
  "x-circle": CircleX,
  "alert-circle": CircleAlert,
  clock: Clock,
  pause: Pause,
  play: Play,
  loader: LoaderCircle,
  ban: Ban,
  mute: BellOff,
  offline: WifiOff,
  // Actions
  copy: Copy,
  external: ExternalLink,
  refresh: RefreshCw,
  more: Ellipsis,
  close: X,
  "chevron-up": ChevronUp,
  "chevron-down": ChevronDown,
  "chevron-left": ChevronLeft,
  "chevron-right": ChevronRight,
  search: Search,
  filter: Funnel,
  plus: Plus,
  trash: Trash2,
  edit: Pencil,
  eye: Eye,
  "eye-off": EyeOff,
  // Theme
  sun: Sun,
  moon: Moon,
  monitor: Monitor,
  // Hints
  info: Info,
  help: CircleQuestionMark,
} as const satisfies Record<string, LucideIcon>;

/** The name of an icon; `dot` is the small filled circle status pills use. */
export type IconName = keyof typeof ICONS | "dot";

/** Every icon name, for the design catalogue. */
export const ICON_NAMES: IconName[] = [...(Object.keys(ICONS) as IconName[]), "dot"];

/** The sizes an icon comes in, in pixels. */
export type IconSize = 14 | 16 | 20;

/**
 * One icon. Decorative by default (hidden from assistive technology, because a label next to
 * it carries the meaning); give `label` when the icon stands alone and it becomes an image
 * with that name.
 */
export function Icon({
  name,
  size = 16,
  label,
  className,
}: {
  className?: string;
  label?: string;
  name: IconName;
  size?: IconSize;
}) {
  const a11y = label
    ? ({ role: "img", "aria-label": label } as const)
    : ({ "aria-hidden": true } as const);
  if (name === "dot") {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 16 16"
        fill="currentColor"
        focusable="false"
        className={className}
        {...a11y}
      >
        <circle cx="8" cy="8" r="3" />
      </svg>
    );
  }
  const Glyph = ICONS[name];
  return (
    <Glyph
      size={size}
      strokeWidth={size === 20 ? 1.75 : 2}
      focusable="false"
      className={className}
      {...a11y}
    />
  );
}
